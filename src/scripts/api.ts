import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Settings = {
  destDir: string;
  parallel: number;
  optimizePng: boolean;
  reencodeVideo: boolean;
  videoCrf: number;
};
export type JobRequest =
  | { kind: 'google'; query: string; limit: number }
  | { kind: 'xLikes'; limit: number }
  | { kind: 'redditSaved'; limit: number };
export type JobStatus = 'queued' | 'running' | 'done' | 'error' | 'cancelled';
export type JobKind = 'google' | 'xLikes' | 'redditSaved';
export type JobInfo = {
  id: string;
  kind: JobKind;
  label: string;
  status: JobStatus;
  found: number;
  saved: number;
  skipped: number;
  failed: number;
  limit: number;
  error: string | null;
  destDir: string;
  startedAt: number | null;
  finishedAt: number | null;
};
export type JobLog = { jobId: string; level: 'info' | 'warn' | 'error'; message: string; ts: number };
export type MediaSaved = {
  jobId: string;
  path: string;
  kind: 'image' | 'video';
  width: number | null;
  height: number | null;
  bytes: number;
};
export type FfmpegStatus = { available: boolean; path: string | null; version: string | null };
export type AccountInfo = { connected: boolean; username: string | null };
export type AccountStatus = { x: AccountInfo; reddit: AccountInfo };
export type Provider = 'x' | 'reddit';

export const inTauri = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Message d'erreur lisible depuis une valeur rejetée par invoke. */
export function errMsg(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

export const getSettings = () => invoke<Settings>('get_settings');
export const saveSettings = (settings: Settings) => invoke<Settings>('save_settings', { settings });
export const queueAdd = (request: JobRequest) => invoke<JobInfo>('queue_add', { request });
export const queueList = () => invoke<JobInfo[]>('queue_list');
export const queueCancel = (id: string) => invoke<null>('queue_cancel', { id });
export const queueClearFinished = () => invoke<null>('queue_clear_finished');
export const recentMedia = (limit: number) => invoke<MediaSaved[]>('recent_media', { limit });
export const ffmpegStatus = () => invoke<FfmpegStatus>('ffmpeg_status');
export const accountStatus = () => invoke<AccountStatus>('account_status');
export const accountLogin = (provider: Provider) => invoke<AccountStatus>('account_login', { provider });
export const accountLogout = (provider: Provider) => invoke<AccountStatus>('account_logout', { provider });

export const onJobUpdate = (cb: (j: JobInfo) => void): Promise<UnlistenFn> =>
  listen<JobInfo>('job-update', (e) => cb(e.payload));
export const onJobLog = (cb: (l: JobLog) => void): Promise<UnlistenFn> =>
  listen<JobLog>('job-log', (e) => cb(e.payload));
export const onMediaSaved = (cb: (m: MediaSaved) => void): Promise<UnlistenFn> =>
  listen<MediaSaved>('media-saved', (e) => cb(e.payload));
export const onAccountChanged = (cb: (a: AccountStatus) => void): Promise<UnlistenFn> =>
  listen<AccountStatus>('account-changed', (e) => cb(e.payload));
