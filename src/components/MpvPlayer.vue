<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import {
  init,
  destroy,
  command,
  observeProperties,
  type MpvConfig,
} from 'tauri-plugin-mpv-api';
import Button from 'primevue/button';

const { t } = useI18n();

const props = defineProps<{
  src: string;
  title: string;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'error', message: string): void;
}>();

const isLoading = ref(true);
const errorMsg = ref<string | null>(null);
const isPlaying = ref(false);

let unlistenProps: (() => void) | null = null;
let healthCheckInterval: ReturnType<typeof setInterval> | null = null;

// Check if mpv is still alive by trying to get a property
async function checkMpvAlive(): Promise<boolean> {
  try {
    await command('get_property', ['pause']);
    return true;
  } catch {
    return false;
  }
}

// Start periodic health check to detect when user closes mpv window
function startHealthCheck() {
  healthCheckInterval = setInterval(async () => {
    const alive = await checkMpvAlive();
    if (!alive) {
      console.log('mpv closed externally, cleaning up');
      stopHealthCheck();
      cleanupAndClose();
    }
  }, 1000); // Check every second
}

function stopHealthCheck() {
  if (healthCheckInterval) {
    clearInterval(healthCheckInterval);
    healthCheckInterval = null;
  }
}

async function cleanupAndClose() {
  stopHealthCheck();
  unlistenProps?.();
  unlistenProps = null;
  try {
    await destroy();
  } catch {
    // Ignore - already dead
  }
  emit('close');
}

async function initMpv() {
  try {
    // Clean up any stale connection first
    try {
      await destroy();
    } catch {
      // Ignore - no previous connection
    }

    // Small delay to ensure socket is cleaned up
    await new Promise(resolve => setTimeout(resolve, 100));

    // Resolve bundled mpv binary and config paths from the Tauri backend
    const mpvPaths = await invoke<{ mpv_path: string | null; config_dir: string | null }>('get_mpv_paths');
    
    console.log('mpvPaths received:', mpvPaths);

    // Build args - use bundled config dir if available
    const args = [
      // IMPORTANT: wid=0 tells mpv to create its own window instead of
      // embedding into the Tauri window (the bundled mpv doesn't support embedding)
      '--wid=0',
      // Video output
      '--vo=gpu-next',
      '--hwdec=auto-safe',
      // Window - force-window is REQUIRED for mpv to open a visible window
      '--force-window=immediate',
      '--geometry=1280x720',
      '--autofit-larger=90%x90%',
      '--keep-open=yes',
      // Disable built-in OSC (we use uosc)
      '--osc=no',
      '--osd-bar=no',
      '--osd-level=1',
      // Cache for streaming
      '--cache=yes',
      '--demuxer-max-bytes=150MiB',
      '--demuxer-max-back-bytes=75MiB',
      // Title
      `--title=${props.title}`,
      `--force-media-title=${props.title}`,
    ];

    // Point mpv to bundled config (includes mpv.conf, uosc scripts, script-opts)
    if (mpvPaths.config_dir) {
      args.push(`--config-dir=${mpvPaths.config_dir}`);
    }

    // Validate mpv path - must use bundled binary, not system PATH
    if (!mpvPaths.mpv_path) {
      throw new Error('Bundled mpv not found. The app may be corrupted - please reinstall.');
    }

    // mpv with uosc - modern beautiful controls IN the player
    const config: MpvConfig = {
      path: mpvPaths.mpv_path,
      args,
      observedProperties: ['pause', 'eof-reached'],
      ipcTimeoutMs: 5000,
    };

    await init(config);
    
    // Observe basic state to update our minimal UI
    unlistenProps = await observeProperties(
      ['pause', 'eof-reached'] as const,
      ({ name, data }) => {
        if (name === 'pause') {
          isPlaying.value = !(data as boolean);
        }
        if (name === 'eof-reached' && data === true) {
          // Video finished
          isPlaying.value = false;
        }
      }
    );

    // Load the video
    await command('loadfile', [props.src]);
    
    isLoading.value = false;
    isPlaying.value = true;
    errorMsg.value = null;
    
    // Start health check to detect when mpv closes
    startHealthCheck();
  } catch (err) {
    console.error('mpv init error:', err);
    let errStr = String(err);
    
    // Provide better guidance for common macOS quarantine issue
    if (errStr.includes('Timed out') && errStr.includes('IPC server')) {
      errStr = t('player.quarantineError') || 
        'mpv no pudo iniciar. En macOS, ejecuta: xattr -cr /Applications/awawapp.app';
    }
    
    errorMsg.value = errStr;
    isLoading.value = false;
    emit('error', errStr);
  }
}

async function closeMpv() {
  stopHealthCheck();
  try {
    await command('quit', []);
  } catch {
    // Ignore - may already be closed
  }
  await cleanupAndClose();
}

onMounted(() => {
  initMpv();
});

onUnmounted(() => {
  stopHealthCheck();
  unlistenProps?.();
  destroy().catch(() => {});
});
</script>

<template>
  <div class="mpv-status">
    <!-- Loading state -->
    <div v-if="isLoading" class="status-card loading">
      <i class="pi pi-spin pi-spinner"></i>
      <span>{{ t('player.launching') || 'Abriendo reproductor...' }}</span>
    </div>

    <!-- Error state -->
    <div v-else-if="errorMsg" class="status-card error">
      <i class="pi pi-exclamation-triangle"></i>
      <p class="error-msg">{{ errorMsg }}</p>
      <p class="hint">
        {{ t('player.installMpvHint') || 'Error al iniciar el reproductor. Reinicia la app o reinstala.' }}
      </p>
      <div class="error-actions">
        <Button 
          :label="t('player.retry') || 'Reintentar'" 
          icon="pi pi-refresh" 
          @click="initMpv" 
          severity="secondary" 
          size="small"
        />
        <Button 
          :label="t('player.close') || 'Cerrar'" 
          icon="pi pi-times" 
          @click="$emit('close')" 
          severity="secondary" 
          text
          size="small"
        />
      </div>
    </div>

    <!-- Playing state - mpv window is open with uosc controls -->
    <div v-else class="status-card playing">
      <div class="now-playing">
        <i class="pi pi-play-circle" :class="{ paused: !isPlaying }"></i>
        <div class="playing-info">
          <span class="playing-label">
            {{ isPlaying ? (t('player.nowPlaying') || 'Reproduciendo') : (t('player.paused') || 'Pausado') }}
          </span>
          <span class="playing-title">{{ title }}</span>
        </div>
      </div>
      <p class="hint">
        {{ t('player.controlsInPlayer') || 'Controles en la ventana del reproductor' }}
      </p>
      <Button 
        :label="t('player.stopPlayback') || 'Detener'" 
        icon="pi pi-stop" 
        @click="closeMpv" 
        severity="danger" 
        outlined
        size="small"
      />
    </div>
  </div>
</template>

<style scoped>
.mpv-status {
  padding: 1rem;
}

.status-card {
  background: var(--surface-card, #1a1612);
  border: 1px solid var(--border-color, #3d352d);
  border-radius: 12px;
  padding: 1.5rem;
  text-align: center;
}

.status-card.loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.75rem;
  color: var(--text-muted, #a09080);
}

.status-card.loading i {
  font-size: 1.25rem;
  color: var(--accent-color, #9d8a78);
}

.status-card.error {
  color: #ff6b6b;
}

.status-card.error i {
  font-size: 2rem;
  margin-bottom: 0.5rem;
}

.error-msg {
  margin: 0.5rem 0;
  font-size: 0.9rem;
}

.hint {
  color: var(--text-muted, #a09080);
  font-size: 0.85rem;
  margin: 0.75rem 0;
}

.hint code {
  background: rgba(255, 255, 255, 0.08);
  padding: 0.2rem 0.5rem;
  border-radius: 4px;
  font-family: monospace;
}

.error-actions {
  display: flex;
  gap: 0.5rem;
  justify-content: center;
  margin-top: 1rem;
}

.status-card.playing {
  border-color: var(--accent-color, #9d8a78);
}

.now-playing {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.75rem;
  margin-bottom: 0.75rem;
}

.now-playing i {
  font-size: 2rem;
  color: #66bb6a;
  transition: color 0.3s;
}

.now-playing i.paused {
  color: var(--accent-color, #9d8a78);
}

.playing-info {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  text-align: left;
}

.playing-label {
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: #66bb6a;
}

.now-playing i.paused + .playing-info .playing-label {
  color: var(--accent-color, #9d8a78);
}

.playing-title {
  font-size: 1rem;
  font-weight: 500;
  color: var(--text-color, #f5f0ea);
  max-width: 300px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
