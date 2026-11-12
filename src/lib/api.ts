// Typed wrappers around the Rust commands.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  AppInfo,
  ArticleDetail,
  ArticleItem,
  Candidate,
  Counts,
  FeedPatch,
  FeedType,
  ItemRef,
  ImportStats,
  ListQuery,
  PeerTubeHit,
  PeerTubeVideo,
  AudioStream,
  Brief,
  BriefInfo,
  ComponentCheck,
  ReaderPage,
  Scope,
  Settings,
  Tree,
  VideoPlayer,
} from './types';

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return invoke<T>(cmd, args);
  // A fake backend so the UI can be developed in a normal browser. Not part of release builds.
  if (import.meta.env.DEV) {
    const mock = await import('./mock');
    return mock.invoke(cmd, args) as Promise<T>;
  }
  throw new Error('Tidings must run inside its desktop app.');
}

export async function on<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  if (inTauri) return listen<T>(event, (e) => handler(e.payload));
  if (import.meta.env.DEV) {
    const mock = await import('./mock');
    return mock.listen(event, handler as (p: unknown) => void);
  }
  return () => {};
}

export const api = {
  tree: () => call<Tree>('get_tree'),
  counts: () => call<Counts>('get_counts'),
  listArticles: (query: ListQuery) => call<ArticleItem[]>('list_articles', { query }),
  listArticlesFull: (query: ListQuery, loadImages?: boolean) =>
    call<ArticleDetail[]>('list_articles_full', { query, loadImages }),
  article: (id: number, loadImages?: boolean, fullText?: boolean) =>
    call<ArticleDetail>('get_article', { id, loadImages, fullText }),
  fullText: (id: number, loadImages?: boolean) =>
    call<ArticleDetail>('fetch_full_text', { id, loadImages }),
  readUrl: (url: string, loadImages?: boolean, reload?: boolean) =>
    call<ReaderPage>('read_url', { url, loadImages, reload }),
  setRead: (ids: number[], read: boolean) => call<void>('set_read', { ids, read }),
  setStarred: (id: number, starred: boolean) => call<void>('set_starred', { id, starred }),
  markAllRead: (scope: Scope, since: number | null = null, until: number | null = null, feedType: FeedType | null = null) =>
    call<number>('mark_all_read', { scope, since, until, feedType }),
  deleteArticles: (ids: number[]) => call<void>('delete_articles', { ids }),
  /** `allowLocal`: the user agreed to connect to this address on the local network. */
  discover: (url: string, allowLocal = false) => call<Candidate[]>('discover_feeds', { url, allowLocal }),
  addFeed: (url: string, folderId: number | null, title?: string, allowLocal = false) =>
    call<number>('add_feed', { url, folderId, title, allowLocal }),
  updateFeed: (id: number, patch: FeedPatch) => call<void>('update_feed', { id, patch }),
  deleteFeed: (id: number) => call<void>('delete_feed', { id }),
  moveFeed: (id: number, folderId: number | null, before: ItemRef | null) =>
    call<void>('move_feed', { id, folderId, before }),
  setPinned: (item: ItemRef, pinned: boolean) => call<void>('set_pinned', { item, pinned }),
  createFolder: (title: string, parentId: number | null) =>
    call<number>('create_folder', { title, parentId }),
  renameFolder: (id: number, title: string) => call<void>('rename_folder', { id, title }),
  deleteFolder: (id: number) => call<void>('delete_folder', { id }),
  moveFolder: (id: number, parentId: number | null, before: ItemRef | null) =>
    call<void>('move_folder', { id, parentId, before }),
  setFolderExpanded: (id: number, expanded: boolean) =>
    call<void>('set_folder_expanded', { id, expanded }),
  setFoldersExpanded: (ids: number[] | null, expanded: boolean) =>
    call<void>('set_folders_expanded', { ids, expanded }),
  deleteItems: (items: ItemRef[]) => call<number>('delete_items', { items }),
  deleteAllSubscriptions: () => call<void>('delete_all_subscriptions'),
  refresh: (opts: { feedIds?: number[]; folderId?: number } = {}) =>
    call<number>('refresh_feeds', { feedIds: opts.feedIds ?? null, folderId: opts.folderId ?? null }),
  importOpml: () => call<ImportStats | null>('import_opml'),
  exportOpml: () => call<string | null>('export_opml'),
  settings: () => call<Settings>('get_settings'),
  saveSettings: (settings: Settings) => call<Settings>('save_settings', { settings }),
  openExternal: (url: string) => call<void>('open_external', { url }),
  videoPlayer: () => call<VideoPlayer | null>('video_player'),
  playVideo: (url: string) => call<void>('play_video', { url }),
  peertubeVideo: (url: string) => call<PeerTubeVideo>('peertube_video', { url }),
  audioStream: (url: string) => call<AudioStream>('audio_stream', { url }),
  aiModels: () => call<string[]>('ai_models'),
  makeBrief: (query: ListQuery, title: string) => call<Brief>('make_brief', { query, title }),
  briefs: () => call<BriefInfo[]>('briefs'),
  brief: (id: number) => call<Brief | null>('brief', { id }),
  deleteBrief: (id: number) => call<void>('delete_brief', { id }),
  /** `run`: may start `yt-dlp --version`. Not at startup. */
  checkComponents: (run: boolean) => call<ComponentCheck[]>('check_components', { run }),
  checkComponentsOnline: () => call<ComponentCheck[]>('check_components_online'),
  searchPeertube: (query: string) => call<PeerTubeHit[]>('search_peertube', { query }),
  setBadge: (count: number) => call<void>('set_badge', { count }),
  info: () => call<AppInfo>('app_info'),
};

export function iconUrl(feedId: number, version: number | null): string | null {
  if (!version || !inTauri) return null;
  return `tidings://localhost/icon/${feedId}?v=${version}`;
}

let mediaBase: string | null = null;

/** Set at startup on Linux, where audio must come from the loopback server. */
export function setMediaBase(base: string | null | undefined) {
  mediaBase = base ?? null;
}

/** Audio is streamed through the Rust media proxy, never fetched by the web view directly. */
export function mediaUrl(url: string): string {
  if (!inTauri) return url;
  const bytes = new TextEncoder().encode(url);
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  const b64 = btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  return mediaBase ? `${mediaBase}/media/${b64}` : `tidings://localhost/media/${b64}`;
}

export function errorText(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err instanceof Error) return err.message;
  return 'Something went wrong';
}
