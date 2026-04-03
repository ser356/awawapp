<script setup lang="ts">
import { ref, computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import type { CommandResult, TorrentInfo } from '../types';
import { isValidMagnetLink } from '../types';
import InputText from 'primevue/inputtext';
import Button from 'primevue/button';

const { t } = useI18n();

const emit = defineEmits<{
  (e: 'torrent-added', info: TorrentInfo): void;
  (e: 'error', message: string): void;
}>();

const magnetLink = ref('');
const isLoading = ref(false);
const errorMessage = ref('');
const fileInput = ref<HTMLInputElement | null>(null);
const isDragging = ref(false);

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
    errorMessage.value = t('magnetInput.invalidMagnet');
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
    errorMessage.value = t('magnetInput.connectionError');
    emit('error', errorMessage.value);
    console.error('Add magnet error:', err);
  } finally {
    isLoading.value = false;
  }
}

async function addTorrentFile(file: File) {
  if (!file.name.endsWith('.torrent')) {
    errorMessage.value = t('magnetInput.onlyTorrentFiles');
    return;
  }
  if (file.size > 10 * 1024 * 1024) {
    errorMessage.value = t('magnetInput.fileTooLarge');
    return;
  }

  isLoading.value = true;
  errorMessage.value = '';
  try {
    const buffer = await file.arrayBuffer();
    const bytes = Array.from(new Uint8Array(buffer));

    const result = await invoke<CommandResult<TorrentInfo>>('add_torrent_file', {
      bytes,
      nameHint: file.name.replace(/\.torrent$/i, ''),
    });

    if (result.success && result.data) {
      emit('torrent-added', result.data);
    } else {
      errorMessage.value = result.error || t('magnetInput.failedToLoadFile');
      emit('error', errorMessage.value);
    }
  } catch {
    errorMessage.value = t('magnetInput.failedToReadFile');
    emit('error', errorMessage.value);
  } finally {
    isLoading.value = false;
  }
}

function openFilePicker() {
  fileInput.value?.click();
}

function onFileSelected(event: Event) {
  const file = (event.target as HTMLInputElement).files?.[0];
  if (file) addTorrentFile(file);
  if (fileInput.value) fileInput.value.value = '';
}

function onDrop(event: DragEvent) {
  isDragging.value = false;
  const file = event.dataTransfer?.files?.[0];
  if (file) addTorrentFile(file);
}

function handlePaste(event: ClipboardEvent) {
  const pastedText = event.clipboardData?.getData('text') || '';
  if (isValidMagnetLink(pastedText.trim())) {
    magnetLink.value = pastedText.trim();
  }
}
</script>

<template>
  <div
    class="magnet-input"
    :class="{ dragging: isDragging }"
    @dragover.prevent="isDragging = true"
    @dragleave.prevent="isDragging = false"
    @drop.prevent="onDrop"
  >
    <div class="input-container">
      <InputText
        v-model="magnetLink"
        :placeholder="t('magnetInput.placeholder')"
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
        :label="t('magnetInput.addTorrent')"
        icon="pi pi-plus"
        class="add-button"
      />
      <Button
        @click="openFilePicker"
        :disabled="isLoading"
        icon="pi pi-file"
        :label="t('magnetInput.torrentFile')"
        severity="secondary"
        outlined
        class="file-button"
        :title="t('magnetInput.torrentFile')"
      />
    </div>

    <p v-if="isDragging" class="drop-hint">Drop .torrent file here</p>
    <p v-else-if="errorMessage" class="error-message">{{ errorMessage }}</p>

    <!-- Hidden file input -->
    <input
      ref="fileInput"
      type="file"
      accept=".torrent"
      style="display: none"
      @change="onFileSelected"
    />
  </div>
</template>

<style scoped>
.magnet-input {
  padding: 1rem;
  background: var(--card-bg, #2a2420);
  border-radius: 12px;
  margin-bottom: 1rem;
  border: 2px solid transparent;
  transition: border-color 0.2s;
}

.magnet-input.dragging {
  border-color: var(--accent-color, #9d8a78);
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

.file-button {
  border-color: var(--border-color, #3d352d) !important;
  color: var(--text-muted, #a09080) !important;
  white-space: nowrap;
}

.file-button:hover:not(:disabled) {
  border-color: var(--accent-color, #9d8a78) !important;
  color: var(--accent-color, #9d8a78) !important;
}

.error-message {
  color: var(--error-color, #c75a5a);
  font-size: 0.875rem;
  margin-top: 0.5rem;
  margin-bottom: 0;
}

.drop-hint {
  color: var(--accent-color, #9d8a78);
  font-size: 0.875rem;
  margin-top: 0.5rem;
  margin-bottom: 0;
  text-align: center;
}
</style>
