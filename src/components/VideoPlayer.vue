<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch, computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import Button from 'primevue/button';
import Slider from 'primevue/slider';
import Dropdown from 'primevue/dropdown';

import type { CommandResult, HlsResult } from '../types';

const { t } = useI18n();

interface AudioTrackInfo {
  index: number;
  label: string;
  language: string;
  enabled: boolean;
}

interface SubtitleTrack {
  label: string;
  src: string;
  default?: boolean;
}

// AudioTrack interface for browsers that support it
interface AudioTrack {
  enabled: boolean;
  id: string;
  kind: string;
  label: string;
  language: string;
}

interface AudioTrackList {
  length: number;
  [index: number]: AudioTrack;
}

const props = defineProps<{
  src: string;
  title: string;
  subtitleFiles?: Array<{ path: string; url: string }>;
  torrentId: number;
  fileIndex: number;
  initialDuration?: number | null;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'error', message: string): void;
  (e: 'update:src', newSrc: string): void;
}>();

// Refs
const videoRef = ref<HTMLVideoElement | null>(null);
const containerRef = ref<HTMLDivElement | null>(null);
const fileInputRef = ref<HTMLInputElement | null>(null);

// State
const isPlaying = ref(false);
const isMuted = ref(false);
const isFullscreen = ref(false);
const isBuffering = ref(true);
const isSeeking = ref(false);
const showControls = ref(true);
const volume = ref(100);
const currentTime = ref(0);
const duration = ref(0);
const buffered = ref(0);
const playbackError = ref<string | null>(null);
const retryCount = ref(0);
const maxRetries = 5;
const currentSrc = ref('');

// Audio tracks
const audioTracks = ref<AudioTrackInfo[]>([]);
const selectedAudioTrack = ref<number>(0);

// Subtitles
const selectedSubtitle = ref<string>('');
const customSubtitles = ref<SubtitleTrack[]>([]);

// Control visibility timer
let controlsTimer: ReturnType<typeof setTimeout> | null = null;

// Computed
const formattedTime = computed(() => formatTime(currentTime.value));
const formattedDuration = computed(() => formatTime(duration.value));
const progressPercent = computed(() => duration.value ? (currentTime.value / duration.value) * 100 : 0);
const bufferedPercent = computed(() => duration.value ? (buffered.value / duration.value) * 100 : 0);

const allSubtitles = computed(() => {
  const subs: Array<{ label: string; value: string }> = [
    { label: t('player.noSubtitles'), value: '' }
  ];
  
  // From torrent files
  if (props.subtitleFiles) {
    props.subtitleFiles.forEach((f, i) => {
      const name = f.path.split('/').pop() || `Subtitle ${i + 1}`;
      subs.push({ label: name, value: f.url });
    });
  }
  
  // Custom loaded subtitles
  customSubtitles.value.forEach(s => {
    subs.push({ label: s.label, value: s.src });
  });
  
  return subs;
});

// Functions
function formatTime(seconds: number): string {
  if (!isFinite(seconds)) return '0:00';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) {
    return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
  }
  return `${m}:${s.toString().padStart(2, '0')}`;
}

function togglePlay() {
  if (!videoRef.value) return;
  if (isPlaying.value) {
    videoRef.value.pause();
  } else {
    videoRef.value.play().catch(handlePlayError);
  }
}

function handlePlayError(err: Error) {
  console.error('Play error:', err);
  if (retryCount.value < maxRetries) {
    retryCount.value++;
    playbackError.value = `Buffering... (retry ${retryCount.value}/${maxRetries})`;
    setTimeout(() => {
      videoRef.value?.play().catch(handlePlayError);
    }, 2000);
  } else {
    playbackError.value = t('player.playbackError');
  }
}

function toggleMute() {
  if (!videoRef.value) return;
  isMuted.value = !isMuted.value;
  videoRef.value.muted = isMuted.value;
}

function setVolume(val: number | number[]) {
  if (!videoRef.value) return;
  const v = Array.isArray(val) ? val[0] : val;
  volume.value = v;
  videoRef.value.volume = v / 100;
  isMuted.value = v === 0;
}

async function seek(percent: number) {
  if (!videoRef.value || !duration.value || isSeeking.value) return;
  
  const targetTime = (percent / 100) * duration.value;
  const bufferEnd = buffered.value;
  
  // If seeking within buffered range, just seek normally
  if (targetTime <= bufferEnd + 10) {
    videoRef.value.currentTime = targetTime;
    return;
  }
  
  // Need to seek beyond buffer - restart transcode from that point
  console.log(`Seek beyond buffer: target=${targetTime}s, buffered=${bufferEnd}s`);
  isSeeking.value = true;
  isBuffering.value = true;
  playbackError.value = t('player.seekingTo', { time: formatTime(targetTime) });
  
  try {
    const result = await invoke<CommandResult<HlsResult>>('seek_transcode', {
      torrentId: props.torrentId,
      fileIndex: props.fileIndex,
      seekTimeSecs: targetTime,
    });
    
    if (result.success && result.data) {
      // Update the video source to the new URL
      currentSrc.value = result.data.url;
      videoRef.value.src = result.data.url;
      videoRef.value.load();
      videoRef.value.play().catch(() => {});
      playbackError.value = null;
    } else {
      playbackError.value = result.error || t('player.seekFailed');
      emit('error', result.error || 'Seek failed');
    }
  } catch (err) {
    console.error('Seek error:', err);
    playbackError.value = t('player.seekFailed');
  } finally {
    isSeeking.value = false;
  }
}

async function seekRelative(seconds: number) {
  if (!videoRef.value) return;
  const targetTime = Math.max(0, Math.min(duration.value, videoRef.value.currentTime + seconds));
  const targetPercent = duration.value ? (targetTime / duration.value) * 100 : 0;
  await seek(targetPercent);
}

async function toggleFullscreen() {
  if (!containerRef.value) return;
  
  if (!document.fullscreenElement) {
    try {
      await containerRef.value.requestFullscreen();
      isFullscreen.value = true;
    } catch (err) {
      console.error('Fullscreen error:', err);
    }
  } else {
    await document.exitFullscreen();
    isFullscreen.value = false;
  }
}

function showControlsTemporarily() {
  showControls.value = true;
  if (controlsTimer) clearTimeout(controlsTimer);
  if (isPlaying.value) {
    controlsTimer = setTimeout(() => {
      showControls.value = false;
    }, 3000);
  }
}

function updateAudioTracks() {
  if (!videoRef.value) return;
  
  const video = videoRef.value as HTMLVideoElement & { audioTracks?: AudioTrackList };
  if (!video.audioTracks) return;
  
  const tracks: AudioTrackInfo[] = [];
  for (let i = 0; i < video.audioTracks.length; i++) {
    const track = video.audioTracks[i];
    tracks.push({
      index: i,
      label: track.label || `Track ${i + 1}`,
      language: track.language || 'Unknown',
      enabled: track.enabled
    });
    if (track.enabled) selectedAudioTrack.value = i;
  }
  audioTracks.value = tracks;
}

function setAudioTrack(index: number) {
  if (!videoRef.value) return;
  
  const video = videoRef.value as HTMLVideoElement & { audioTracks?: AudioTrackList };
  if (!video.audioTracks) return;
  
  for (let i = 0; i < video.audioTracks.length; i++) {
    video.audioTracks[i].enabled = (i === index);
  }
  selectedAudioTrack.value = index;
}

// SRT to VTT conversion
function srtToVtt(srt: string): string {
  let vtt = 'WEBVTT\n\n';
  
  // Split into subtitle blocks
  const blocks = srt.trim().split(/\n\s*\n/);
  
  for (const block of blocks) {
    const lines = block.split('\n');
    if (lines.length < 2) continue;
    
    // Find timestamp line (might be first or second line)
    let timestampIndex = 0;
    if (!/-->/.test(lines[0])) timestampIndex = 1;
    if (timestampIndex >= lines.length) continue;
    
    // Convert timestamp format: 00:00:00,000 -> 00:00:00.000
    const timestamp = lines[timestampIndex].replace(/,/g, '.');
    
    // Get text (remaining lines)
    const text = lines.slice(timestampIndex + 1).join('\n');
    
    if (timestamp && text) {
      vtt += `${timestamp}\n${text}\n\n`;
    }
  }
  
  return vtt;
}

async function loadSubtitleFile(file: File) {
  try {
    const text = await file.text();
    let vttContent = text;
    
    // Convert SRT to VTT if needed
    if (file.name.toLowerCase().endsWith('.srt')) {
      vttContent = srtToVtt(text);
    }
    
    // Create blob URL for the VTT content
    const blob = new Blob([vttContent], { type: 'text/vtt' });
    const url = URL.createObjectURL(blob);
    
    const track: SubtitleTrack = {
      label: file.name,
      src: url
    };
    
    customSubtitles.value.push(track);
    selectedSubtitle.value = url;
    applySubtitle(url);
    
  } catch (err) {
    console.error('Failed to load subtitle:', err);
    emit('error', 'Failed to load subtitle file');
  }
}

function applySubtitle(url: string) {
  if (!videoRef.value) return;
  
  // Remove existing subtitle tracks
  const existingTracks = videoRef.value.querySelectorAll('track[data-custom]');
  existingTracks.forEach(t => t.remove());
  
  // Disable all text tracks
  for (let i = 0; i < videoRef.value.textTracks.length; i++) {
    videoRef.value.textTracks[i].mode = 'disabled';
  }
  
  if (!url) return;
  
  // Add new track
  const track = document.createElement('track');
  track.kind = 'subtitles';
  track.src = url;
  track.default = true;
  track.setAttribute('data-custom', 'true');
  videoRef.value.appendChild(track);
  
  // Enable the new track
  setTimeout(() => {
    if (videoRef.value && videoRef.value.textTracks.length > 0) {
      const lastTrack = videoRef.value.textTracks[videoRef.value.textTracks.length - 1];
      lastTrack.mode = 'showing';
    }
  }, 100);
}

function openSubtitlePicker() {
  fileInputRef.value?.click();
}

function handleSubtitleFileChange(event: Event) {
  const input = event.target as HTMLInputElement;
  if (input.files && input.files[0]) {
    loadSubtitleFile(input.files[0]);
  }
  input.value = ''; // Reset for re-selection
}

function handleDrop(event: DragEvent) {
  event.preventDefault();
  const files = event.dataTransfer?.files;
  if (!files) return;
  
  for (const file of files) {
    const name = file.name.toLowerCase();
    if (name.endsWith('.srt') || name.endsWith('.vtt') || name.endsWith('.sub')) {
      loadSubtitleFile(file);
      break;
    }
  }
}

function handleDragOver(event: DragEvent) {
  event.preventDefault();
}

// Keyboard shortcuts
function handleKeydown(event: KeyboardEvent) {
  if (event.target instanceof HTMLInputElement) return;
  
  switch (event.code) {
    case 'Space':
      event.preventDefault();
      togglePlay();
      break;
    case 'ArrowLeft':
      event.preventDefault();
      seekRelative(-10);
      break;
    case 'ArrowRight':
      event.preventDefault();
      seekRelative(10);
      break;
    case 'ArrowUp':
      event.preventDefault();
      setVolume(Math.min(100, volume.value + 10));
      break;
    case 'ArrowDown':
      event.preventDefault();
      setVolume(Math.max(0, volume.value - 10));
      break;
    case 'KeyF':
      event.preventDefault();
      toggleFullscreen();
      break;
    case 'KeyM':
      event.preventDefault();
      toggleMute();
      break;
    case 'Escape':
      if (isFullscreen.value) {
        document.exitFullscreen();
      } else {
        emit('close');
      }
      break;
  }
}

// Event handlers
function onTimeUpdate() {
  if (!videoRef.value) return;
  currentTime.value = videoRef.value.currentTime;
  
  // Update buffered
  if (videoRef.value.buffered.length > 0) {
    buffered.value = videoRef.value.buffered.end(videoRef.value.buffered.length - 1);
  }
}

function onLoadedMetadata() {
  if (!videoRef.value) return;
  duration.value = videoRef.value.duration;
  updateAudioTracks();
  isBuffering.value = false;
  playbackError.value = null;
  retryCount.value = 0;
}

function onPlay() {
  isPlaying.value = true;
  showControlsTemporarily();
}

function onPause() {
  isPlaying.value = false;
  showControls.value = true;
}

function onWaiting() {
  isBuffering.value = true;
}

function onCanPlay() {
  isBuffering.value = false;
  playbackError.value = null;
}

function onError(event: Event) {
  const video = event.target as HTMLVideoElement;
  const error = video.error;
  
  console.error('Video error:', error);
  
  if (retryCount.value < maxRetries) {
    retryCount.value++;
    playbackError.value = `Connection lost. Retrying... (${retryCount.value}/${maxRetries})`;
    
    setTimeout(() => {
      if (videoRef.value) {
        const currentPos = videoRef.value.currentTime;
        videoRef.value.load();
        videoRef.value.currentTime = currentPos;
        videoRef.value.play().catch(() => {});
      }
    }, 2000);
  } else {
    playbackError.value = t('player.connectionLost');
  }
}

function onFullscreenChange() {
  isFullscreen.value = !!document.fullscreenElement;
}

// Watch subtitle selection
watch(selectedSubtitle, (url) => {
  applySubtitle(url);
});

// Lifecycle
onMounted(() => {
  document.addEventListener('keydown', handleKeydown);
  document.addEventListener('fullscreenchange', onFullscreenChange);
  
  // Initialize current source
  currentSrc.value = props.src;
  
  // Use initial duration from backend (ffprobe) if available
  if (props.initialDuration && props.initialDuration > 0) {
    duration.value = props.initialDuration;
    console.log('Using initial duration from backend:', props.initialDuration);
  }
  
  // Auto-play when mounted
  if (videoRef.value) {
    videoRef.value.play().catch(() => {
      // Autoplay blocked, user needs to interact
      isBuffering.value = false;
    });
  }
});

onUnmounted(() => {
  document.removeEventListener('keydown', handleKeydown);
  document.removeEventListener('fullscreenchange', onFullscreenChange);
  if (controlsTimer) clearTimeout(controlsTimer);
  
  // Cleanup blob URLs
  customSubtitles.value.forEach(s => {
    if (s.src.startsWith('blob:')) {
      URL.revokeObjectURL(s.src);
    }
  });
});
</script>

<template>
  <div 
    ref="containerRef"
    class="video-player"
    @mousemove="showControlsTemporarily"
    @drop="handleDrop"
    @dragover="handleDragOver"
  >
    <!-- Video Element -->
    <video
      ref="videoRef"
      :src="currentSrc || src"
      class="video-element"
      @timeupdate="onTimeUpdate"
      @loadedmetadata="onLoadedMetadata"
      @play="onPlay"
      @pause="onPause"
      @waiting="onWaiting"
      @canplay="onCanPlay"
      @error="onError"
      @click="togglePlay"
      @dblclick="toggleFullscreen"
      preload="auto"
    />
    
    <!-- Buffering/Seeking Indicator -->
    <div v-if="isBuffering || isSeeking" class="buffering-overlay">
      <div class="spinner"></div>
      <span v-if="playbackError">{{ playbackError }}</span>
      <span v-else-if="isSeeking">{{ t('player.seekingTo', { time: '' }) }}</span>
      <span v-else>{{ t('player.buffering') }}</span>
    </div>
    
    <!-- Error Overlay -->
    <div v-if="playbackError && !isBuffering" class="error-overlay">
      <i class="pi pi-exclamation-triangle"></i>
      <p>{{ playbackError }}</p>
      <Button 
        :label="t('player.retry')" 
        @click="retryCount = 0; videoRef?.load(); videoRef?.play()"
        severity="secondary"
      />
    </div>
    
    <!-- Controls -->
    <div 
      class="controls-wrapper"
      :class="{ visible: showControls || !isPlaying }"
    >
      <!-- Top Bar -->
      <div class="top-bar">
        <Button
          icon="pi pi-arrow-left"
          text
          rounded
          @click="emit('close')"
          class="back-btn"
        />
        <h3 class="video-title">{{ title }}</h3>
        <div class="spacer"></div>
      </div>
      
      <!-- Bottom Controls -->
      <div class="bottom-controls">
        <!-- Progress Bar -->
        <div 
          class="progress-container" 
          @click="(e) => seek((e.offsetX / (e.target as HTMLElement).clientWidth) * 100)"
        >
          <div class="progress-buffered" :style="{ width: bufferedPercent + '%' }"></div>
          <div class="progress-played" :style="{ width: progressPercent + '%' }"></div>
          <div class="progress-handle" :style="{ left: progressPercent + '%' }"></div>
          <!-- Seek zone indicator -->
          <div v-if="bufferedPercent < 100" class="progress-seek-zone" :style="{ left: bufferedPercent + '%' }">
            <span class="seek-zone-label">{{ t('player.seekFar') }}</span>
          </div>
        </div>
        
        <!-- Control Buttons -->
        <div class="controls-row">
          <div class="left-controls">
            <!-- Play/Pause -->
            <Button
              :icon="isPlaying ? 'pi pi-pause' : 'pi pi-play'"
              text
              rounded
              @click="togglePlay"
            />
            
            <!-- Skip buttons -->
            <Button
              icon="pi pi-replay"
              text
              rounded
              @click="seekRelative(-10)"
              v-tooltip.top="'-10s'"
            />
            <Button
              icon="pi pi-forward"
              text
              rounded
              @click="seekRelative(10)"
              v-tooltip.top="'+10s'"
            />
            
            <!-- Volume -->
            <div class="volume-control">
              <Button
                :icon="isMuted || volume === 0 ? 'pi pi-volume-off' : 'pi pi-volume-up'"
                text
                rounded
                @click="toggleMute"
              />
              <Slider 
                v-model="volume" 
                class="volume-slider"
                @update:modelValue="setVolume"
              />
            </div>
            
            <!-- Time -->
            <span class="time-display">
              {{ formattedTime }} / {{ formattedDuration }}
            </span>
          </div>
          
          <div class="right-controls">
            <!-- Audio Tracks -->
            <Dropdown
              v-if="audioTracks.length > 1"
              v-model="selectedAudioTrack"
              :options="audioTracks"
              optionLabel="label"
              optionValue="index"
              @change="(e: { value: number }) => setAudioTrack(e.value)"
              class="track-dropdown"
              :placeholder="t('player.audioTrack')"
            >
              <template #value="{ value }">
                <span class="dropdown-value">
                  <i class="pi pi-volume-up"></i>
                  {{ audioTracks[value]?.label || t('player.audioTrack') }}
                </span>
              </template>
            </Dropdown>
            
            <!-- Subtitles -->
            <div class="subtitle-controls">
              <Dropdown
                v-model="selectedSubtitle"
                :options="allSubtitles"
                optionLabel="label"
                optionValue="value"
                class="track-dropdown"
                :placeholder="t('player.subtitles')"
              >
                <template #value="{ value }">
                  <span class="dropdown-value">
                    <i class="pi pi-comment"></i>
                    {{ allSubtitles.find(s => s.value === value)?.label || t('player.subtitles') }}
                  </span>
                </template>
              </Dropdown>
              
              <Button
                icon="pi pi-folder-open"
                text
                rounded
                @click="openSubtitlePicker"
                v-tooltip.top="t('player.loadSubtitle')"
              />
            </div>
            
            <!-- Fullscreen -->
            <Button
              :icon="isFullscreen ? 'pi pi-window-minimize' : 'pi pi-window-maximize'"
              text
              rounded
              @click="toggleFullscreen"
            />
          </div>
        </div>
      </div>
    </div>
    
    <!-- Hidden file input for subtitles -->
    <input
      ref="fileInputRef"
      type="file"
      accept=".srt,.vtt,.sub"
      style="display: none"
      @change="handleSubtitleFileChange"
    />
  </div>
</template>

<style scoped>
.video-player {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: #000;
  z-index: 1000;
  display: flex;
  flex-direction: column;
}

.video-element {
  width: 100%;
  height: 100%;
  object-fit: contain;
  cursor: pointer;
}

/* Subtitles styling */
.video-element::cue {
  background: rgba(0, 0, 0, 0.7);
  color: white;
  font-size: 1.2em;
  font-family: inherit;
  text-shadow: 1px 1px 2px black;
}

.buffering-overlay,
.error-overlay {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1rem;
  color: white;
  text-align: center;
  z-index: 10;
}

.spinner {
  width: 50px;
  height: 50px;
  border: 3px solid rgba(255, 255, 255, 0.2);
  border-top-color: white;
  border-radius: 50%;
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.error-overlay i {
  font-size: 3rem;
  color: #ff6b6b;
}

.controls-wrapper {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  opacity: 0;
  transition: opacity 0.3s ease;
  pointer-events: none;
}

.controls-wrapper.visible {
  opacity: 1;
  pointer-events: auto;
}

.top-bar {
  display: flex;
  align-items: center;
  padding: 1rem 1.5rem;
  background: linear-gradient(to bottom, rgba(0, 0, 0, 0.7), transparent);
}

.back-btn {
  color: white !important;
}

.video-title {
  margin: 0 0 0 1rem;
  font-size: 1.1rem;
  font-weight: 500;
  color: white;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 60%;
}

.spacer {
  flex: 1;
}

.bottom-controls {
  background: linear-gradient(to top, rgba(0, 0, 0, 0.8), transparent);
  padding: 1rem 1.5rem 1.5rem;
}

.progress-container {
  position: relative;
  height: 4px;
  background: rgba(255, 255, 255, 0.2);
  border-radius: 2px;
  cursor: pointer;
  margin-bottom: 1rem;
}

.progress-container:hover {
  height: 6px;
}

.progress-buffered {
  position: absolute;
  top: 0;
  left: 0;
  height: 100%;
  background: rgba(255, 255, 255, 0.3);
  border-radius: 2px;
}

.progress-played {
  position: absolute;
  top: 0;
  left: 0;
  height: 100%;
  background: var(--primary-color, #c9a882);
  border-radius: 2px;
}

.progress-handle {
  position: absolute;
  top: 50%;
  transform: translate(-50%, -50%);
  width: 12px;
  height: 12px;
  background: white;
  border-radius: 50%;
  opacity: 0;
  transition: opacity 0.2s;
}

.progress-container:hover .progress-handle {
  opacity: 1;
}

.progress-seek-zone {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  right: 0;
  height: 100%;
  pointer-events: none;
}

.seek-zone-label {
  display: none;
  position: absolute;
  top: -30px;
  left: 50%;
  transform: translateX(-50%);
  background: rgba(0, 0, 0, 0.8);
  color: white;
  font-size: 0.7rem;
  padding: 4px 8px;
  border-radius: 4px;
  white-space: nowrap;
}

.progress-container:hover .seek-zone-label {
  display: block;
}

.controls-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.left-controls,
.right-controls {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.left-controls :deep(.p-button) {
  color: white !important;
}

.right-controls :deep(.p-button) {
  color: white !important;
}

.volume-control {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.volume-slider {
  width: 80px;
}

.volume-slider :deep(.p-slider-range) {
  background: var(--primary-color, #c9a882);
}

.volume-slider :deep(.p-slider-handle) {
  background: white;
  border: none;
}

.time-display {
  color: white;
  font-size: 0.85rem;
  font-variant-numeric: tabular-nums;
  margin-left: 0.5rem;
}

.track-dropdown {
  background: rgba(255, 255, 255, 0.1) !important;
  border: none !important;
  min-width: 120px;
}

.track-dropdown :deep(.p-dropdown-label) {
  color: white !important;
  padding: 0.4rem 0.75rem;
  font-size: 0.85rem;
}

.track-dropdown :deep(.p-dropdown-trigger) {
  color: white !important;
}

.dropdown-value {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.subtitle-controls {
  display: flex;
  align-items: center;
  gap: 0.25rem;
}

/* Fullscreen adjustments */
.video-player:fullscreen .video-title {
  font-size: 1.3rem;
}

.video-player:fullscreen .progress-container {
  height: 6px;
}

.video-player:fullscreen .controls-row {
  padding: 0 1rem;
}
</style>
