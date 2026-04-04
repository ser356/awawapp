// TypeScript types matching the Rust backend
// Security note: All types are validated on the backend, but TypeScript provides
// additional compile-time safety

export interface CommandResult<T> {
  success: boolean;
  data: T | null;
  error: string | null;
}

export interface TorrentFile {
  index: number;
  path: string;
  size: number;
  selected: boolean;
}

export interface TorrentInfo {
  id: number;
  name: string;
  files: TorrentFile[];
  total_size: number;
}

export interface TorrentStats {
  id: number;
  name: string;
  progress: number;
  download_speed: number;
  upload_speed: number;
  peers_connected: number;
  peers_total: number;
  downloaded_bytes: number;
  total_bytes: number;
  state: string;
  eta_seconds: number | null;
}

export interface TorrentHistory {
  id: number;
  magnet_link: string;
  name: string;
  added_at: string;
  total_size: number;
  status: string;
}

export interface HlsResult {
  url: string;
  duration_secs: number | null;
}

// Utility functions for formatting
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
}

export function formatSpeed(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatEta(seconds: number | null): string {
  if (seconds === null || seconds === 0) return '--';
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return `${hours}h ${minutes}m`;
}

export function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString('es-ES', {
    day: '2-digit',
    month: 'short',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit'
  });
}

// Validation functions
export function isValidMagnetLink(uri: string): boolean {
  if (!uri.startsWith('magnet:?')) return false;
  if (!uri.includes('xt=urn:btih:') && !uri.includes('xt=urn:btmh:')) return false;
  if (uri.length > 10000) return false;
  return true;
}
