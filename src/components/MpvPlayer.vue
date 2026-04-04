<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  init,
  destroy,
  command,
  setProperty,
  getProperty,
  observeProperties,
  type MpvConfig,
} from 'tauri-plugin-mpv-api';
import Button from 'primevue/button';
import Slider from 'primevue/slider';

const { t } = useI18n();

const props = defineProps<{
  src: string;
  title: string;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'error', message: string): void;
}>();

// Reactive state
const isPlaying = ref(false);
const currentTime = ref(0);
const duration = ref(0);
const volume = ref(100);
const isReady = ref(false);
const errorMsg = ref<string | null>(null);
const isInitializing = ref(true);

// Computed
const formattedTime = computed(() => formatTime(currentTime.value));
const formattedDuration = computed(() => formatTime(duration.value));
const progressPercent = computed(() =>
  duration.value > 0 ? (currentTime.value / duration.value) * 100 : 0
);

const OBSERVED_PROPS = [
  'pause',
  'time-pos',
  'duration',
  'volume',
  'eof-reached',
] as const;

function formatTime(secs: number): string {
  if (!isFinite(secs) || secs < 0) return '0:00';
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  if (h > 0) return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
  return `${m}:${s.toString().padStart(2, '0')}`;
}

let unlistenProps: (() => void) | null = null;

async function initMpv() {
  try {
    const config: MpvConfig = {
      args: [
        '--vo=gpu-next',
        '--hwdec=auto-safe',
        '--keep-open=yes',
        '--force-window=yes',
        '--osc=yes',
        '--title=' + props.title,
        '--volume=100',
        '--cache=yes',
        '--demuxer-max-bytes=150MiB',
        '--demuxer-max-back-bytes=75MiB',
      ],
      observedProperties: OBSERVED_PROPS,
      ipcTimeoutMs: 5000,
    };

    await init(config);
    isReady.value = true;
    isInitializing.value = false;

    // Observe properties in real time
    unlistenProps = await observeProperties(
      OBSERVED_PROPS,
      ({ name, data }) => {
        switch (name) {
          case 'pause':
            isPlaying.value = !(data as boolean);
            break;
          case 'time-pos':
            if (typeof data === 'number') currentTime.value = data;
            break;
          case 'duration':
            if (typeof data === 'number') duration.value = data;
            break;
          case 'volume':
            if (typeof data === 'number') volume.value = data;
            break;
          case 'eof-reached':
            if (data === true) isPlaying.value = false;
            break;
        }
      }
    );

    // Load the stream URL directly — no transcoding needed, mpv eats everything
    await command('loadfile', [props.src]);
    errorMsg.value = null;
  } catch (err) {
    console.error('mpv init error:', err);
    errorMsg.value = String(err);
    isInitializing.value = false;
    emit('error', String(err));
  }
}

// === Controls ===

async function togglePlay() {
  try {
    const paused = await getProperty('pause');
    await setProperty('pause', !paused);
  } catch (err) {
    console.error('togglePlay:', err);
  }
}

async function seek(percent: number) {
  if (!duration.value) return;
  const target = (percent / 100) * duration.value;
  try {
    await command('seek', [target.toString(), 'absolute']);
  } catch (err) {
    console.error('seek:', err);
  }
}

async function seekRelative(seconds: number) {
  try {
    await command('seek', [seconds.toString(), 'relative']);
  } catch (err) {
    console.error('seekRelative:', err);
  }
}

async function setVol(val: number | number[]) {
  const v = Array.isArray(val) ? val[0] : val;
  volume.value = v;
  try {
    await setProperty('volume', v);
  } catch (err) {
    console.error('setVolume:', err);
  }
}

async function toggleFullscreen() {
  try {
    const fs = await getProperty('fullscreen');
    await setProperty('fullscreen', !fs);
  } catch (err) {
    console.error('fullscreen:', err);
  }
}

async function closeMpv() {
  try {
    await destroy();
  } catch {
    // ignore — mpv may already be closed
  }
  emit('close');
}

// Keyboard shortcuts
function handleKeydown(e: KeyboardEvent) {
  if (e.target instanceof HTMLInputElement) return;
  switch (e.code) {
    case 'Space':     e.preventDefault(); togglePlay(); break;
    case 'ArrowLeft': e.preventDefault(); seekRelative(-10); break;
    case 'ArrowRight':e.preventDefault(); seekRelative(10); break;
    case 'ArrowUp':   e.preventDefault(); setVol(Math.min(150, volume.value + 5)); break;
    case 'ArrowDown': e.preventDefault(); setVol(Math.max(0, volume.value - 5)); break;
    case 'KeyF':      e.preventDefault(); toggleFullscreen(); break;
    case 'Escape':    closeMpv(); break;
  }
}

onMounted(() => {
  document.addEventListener('keydown', handleKeydown);
  initMpv();
});

onUnmounted(() => {
  document.removeEventListener('keydown', handleKeydown);
  unlistenProps?.();
  destroy().catch(() => {});
});
</script>

<template>
  <div class="mpv-controller">
    <!-- Header -->
    <div class="controller-header">
      <Button icon="pi pi-arrow-left" text rounded @click="closeMpv" class="ctrl-btn" />
      <div class="title-area">
        <h3 class="video-title">{{ title }}</h3>
        <span class="status" :class="{ playing: isPlaying, init: isInitializing }">
          <template v-if="isInitializing">⏳ {{ t('player.buffering') }}</template>
          <template v-else-if="errorMsg">❌ Error</template>
          <template v-else-if="isPlaying">▶ {{ t('player.playing') || 'Reproduciendo en mpv' }}</template>
          <template v-else>⏸ {{ t('player.paused') || 'Pausado' }}</template>
        </span>
      </div>
    </div>

    <!-- Error state -->
    <div v-if="errorMsg" class="error-panel">
      <i class="pi pi-exclamation-triangle"></i>
      <p>{{ errorMsg }}</p>
      <p class="hint">{{ t('player.installMpvHint') || 'Asegúrate de tener mpv instalado:' }} <code>brew install mpv</code></p>
      <Button :label="t('player.retry') || 'Reintentar'" icon="pi pi-refresh" @click="initMpv" severity="secondary" />
    </div>

    <!-- Controls -->
    <div v-else class="controls">
      <!-- Progress bar -->
      <div
        class="progress-container"
        @click="(e: MouseEvent) => seek((e.offsetX / (e.currentTarget as HTMLElement).clientWidth) * 100)"
      >
        <div class="progress-played" :style="{ width: progressPercent + '%' }"></div>
        <div class="progress-handle" :style="{ left: progressPercent + '%' }"></div>
      </div>

      <!-- Buttons -->
      <div class="controls-row">
        <div class="left-controls">
          <Button
            :icon="isPlaying ? 'pi pi-pause' : 'pi pi-play'"
            text rounded @click="togglePlay" class="ctrl-btn"
          />
          <Button icon="pi pi-replay" text rounded @click="seekRelative(-10)" class="ctrl-btn" v-tooltip.top="'-10s'" />
          <Button icon="pi pi-forward" text rounded @click="seekRelative(10)" class="ctrl-btn" v-tooltip.top="'+10s'" />

          <div class="volume-control">
            <Button
              :icon="volume === 0 ? 'pi pi-volume-off' : 'pi pi-volume-up'"
              text rounded @click="setVol(volume === 0 ? 100 : 0)" class="ctrl-btn"
            />
            <Slider v-model="volume" :max="150" class="volume-slider" @update:modelValue="setVol" />
          </div>

          <span class="time-display">{{ formattedTime }} / {{ formattedDuration }}</span>
        </div>

        <div class="right-controls">
          <Button icon="pi pi-window-maximize" text rounded @click="toggleFullscreen" class="ctrl-btn" v-tooltip.top="'Fullscreen (F)'" />
          <Button icon="pi pi-times" text rounded @click="closeMpv" class="ctrl-btn close-btn" v-tooltip.top="'Cerrar (Esc)'" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.mpv-controller {
  background: var(--surface-card, #1a1612);
  border: 1px solid var(--border-color, #3d352d);
  border-radius: 12px;
  padding: 1.25rem;
  margin: 1rem;
  transition: all 0.3s ease;
}

.controller-header {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  margin-bottom: 1rem;
}

.title-area {
  flex: 1;
  min-width: 0;
}

.video-title {
  margin: 0;
  font-size: 1rem;
  font-weight: 500;
  color: var(--text-color, #f5f0ea);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.status {
  font-size: 0.8rem;
  color: var(--text-muted, #a09080);
  transition: color 0.3s;
}
.status.playing { color: #66bb6a; }
.status.init { color: #ffa726; }

.ctrl-btn {
  color: var(--text-color, #f5f0ea) !important;
}
.close-btn:hover {
  color: #ff6b6b !important;
}

/* Error */
.error-panel {
  text-align: center;
  padding: 2rem 1rem;
  color: #ff6b6b;
}
.error-panel .hint {
  color: var(--text-muted);
  font-size: 0.85rem;
  margin: 0.5rem 0 1rem;
}
.error-panel code {
  background: rgba(255,255,255,0.08);
  padding: 0.2rem 0.5rem;
  border-radius: 4px;
  font-size: 0.85rem;
}

/* Progress bar */
.progress-container {
  position: relative;
  height: 4px;
  background: rgba(255, 255, 255, 0.12);
  border-radius: 2px;
  cursor: pointer;
  margin-bottom: 1rem;
  transition: height 0.15s;
}
.progress-container:hover { height: 6px; }

.progress-played {
  position: absolute;
  top: 0; left: 0;
  height: 100%;
  background: var(--primary-color, #c9a882);
  border-radius: 2px;
  transition: width 0.1s linear;
}

.progress-handle {
  position: absolute;
  top: 50%;
  transform: translate(-50%, -50%);
  width: 14px; height: 14px;
  background: white;
  border-radius: 50%;
  opacity: 0;
  transition: opacity 0.15s;
  box-shadow: 0 1px 4px rgba(0,0,0,0.4);
}
.progress-container:hover .progress-handle { opacity: 1; }

/* Controls row */
.controls-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.left-controls, .right-controls {
  display: flex;
  align-items: center;
  gap: 0.4rem;
}

.volume-control {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.volume-slider { width: 80px; }
.volume-slider :deep(.p-slider-range) { background: var(--primary-color, #c9a882); }
.volume-slider :deep(.p-slider-handle) { background: white; border: none; width: 12px; height: 12px; }

.time-display {
  color: var(--text-muted, #a09080);
  font-size: 0.85rem;
  font-variant-numeric: tabular-nums;
  margin-left: 0.5rem;
  user-select: none;
}
</style>
