export type Scope =
  | { kind: 'all' }
  | { kind: 'starred' }
  | { kind: 'feed'; id: number }
  | { kind: 'folder'; id: number }
  | { kind: 'type'; type: FeedType };

/** Detected from the feed's address and enclosures, or chosen by the user. */
export type FeedType = 'podcast' | 'video' | 'code' | 'discussion' | 'newsletter' | 'article';

/** Which publication dates to show. Custom dates are local `YYYY-MM-DD`; `to` is inclusive. */
export type DateRange =
  | { preset: 'any' | 'today' | 'yesterday' | 'week' | 'month' | 'year' | 'older' }
  | { preset: 'custom'; from: string | null; to: string | null };

export type Filter = 'all' | 'unread' | 'new' | 'starred';

export interface Folder {
  id: number;
  parentId: number | null;
  title: string;
  position: number;
  expanded: boolean;
  /** Unix time it was added to Favorites, 0 if not a favorite. */
  pinned: number;
}

export interface Feed {
  id: number;
  folderId: number | null;
  title: string;
  feedTitle: string;
  customTitle: string | null;
  url: string;
  siteUrl: string | null;
  description: string | null;
  position: number;
  iconVersion: number | null;
  updateInterval: number | null;
  retentionMode: string | null;
  retentionValue: number | null;
  fullText: boolean;
  paused: boolean;
  lastFetched: number | null;
  lastSuccess: number | null;
  lastError: string | null;
  unread: number;
  total: number;
  /** Unix time it was added to Favorites, 0 if not a favorite. */
  pinned: number;
  /** The type in use: the user's choice, else the detected one. */
  feedType: FeedType;
  /** The user's choice, null for automatic. */
  typeOverride: FeedType | null;
  detectedType: FeedType;
  /** The user allowed Tidings to fetch this feed from the local network. */
  local: boolean;
  /** Unix time it was added. */
  createdAt: number;
  /** Publication date of its newest article (Unix seconds), null when it has none. */
  newest: number | null;
}

/** How the sidebar orders subscriptions. `manual` is the order made by dragging. */
export type TreeSortKey = 'manual' | 'name' | 'added' | 'unread' | 'latest';

export interface TreeSort {
  key: TreeSortKey;
  descending: boolean;
  /** Sort feeds as well as folders. When false, feeds keep their manual order. */
  feeds: boolean;
}

export interface Tree {
  folders: Folder[];
  feeds: Feed[];
  starred: number;
}

export interface Counts {
  feeds: [number, number][];
  starred: number;
}

export interface ArticleItem {
  id: number;
  feedId: number;
  title: string;
  url: string | null;
  author: string | null;
  excerpt: string | null;
  published: number;
  isRead: boolean;
  isStarred: boolean;
  isNew: boolean;
  hasEnclosure: boolean;
  thumb: string | null;
}

export interface ArticleDetail {
  id: number;
  feedId: number;
  title: string;
  url: string | null;
  author: string | null;
  published: number;
  isRead: boolean;
  isStarred: boolean;
  isNew: boolean;
  html: string;
  blockedImages: number;
  showingFullText: boolean;
  hasFullText: boolean;
  enclosureUrl: string | null;
  enclosureType: string | null;
  /** Mostly Chinese. */
  chinese: boolean;
  /** "zh" (Chinese), "ar" (Arabic) or "" (another language), for the translations offered. */
  language: '' | 'zh' | 'ar';
  /** Where a translation's paragraphs and the article's images go (articles that can be translated). */
  translationLayout: TranslationBlock[];
}

/** A paragraph of a translation by its index, or one of the article's images as cleaned HTML. */
export type TranslationBlock = { kind: 'text'; index: number } | { kind: 'image'; html: string };

/** A feed's newest article and article count, for Feed health. */
/** What a backup holds, shown before it is restored (backup.rs). */
export interface BackupSummary {
  fileName: string;
  /** When it was made (Unix seconds) and by which version; null for a copy of the database file. */
  createdAt: number | null;
  version: string | null;
  folders: number;
  feeds: number;
  articles: number;
  unread: number;
  starred: number;
  rules: number;
  searches: number;
  /** Host and port of the feeds allowed on the local network. They keep that permission. */
  localFeeds: string[];
  /** The proxy now and after the restore, without passwords. */
  proxyNow: string;
  proxyAfter: string;
  /** Set when the restored settings cannot be used: Tidings goes offline. */
  offline: string | null;
}

export interface RestoreResult {
  /** Where the data from before the restore is kept. */
  previous: string;
  offline: string | null;
}

export interface FeedActivity {
  feedId: number;
  newest: number | null;
  articles: number;
}

/** The list state a saved search restores. */
export interface SearchState {
  scope: Scope;
  filter: Filter;
  query: string;
  dateRange: DateRange;
  typeFilter: FeedType | null;
}

/** A saved search in the sidebar; `state` is a SearchState as JSON. */
export interface SavedSearch {
  id: number;
  title: string;
  state: string;
}

/** A filter rule (rules.rs): when `field` contains one of `words`, do `action`. */
export interface Rule {
  /** 0 for a rule that is not saved yet. */
  id: number;
  enabled: boolean;
  /** The feed or folder the rule is for; neither: all feeds. */
  feedId: number | null;
  folderId: number | null;
  field: 'title' | 'text' | 'author' | 'link';
  /** Phrases separated by commas. */
  words: string;
  action: 'read' | 'star' | 'delete';
  /** Articles the rule matched so far. */
  hits: number;
}

/** The language of a translation: English or Arabic. */
export type TranslateLang = 'en' | 'ar';

/** An article translated by the local model (translate.rs). */
export interface Translation {
  title: string;
  paragraphs: string[];
  model: string;
  /** The article was longer than what was translated. */
  cut: boolean;
  lang: TranslateLang;
}

/** One part of a translation while the model works ("translation-part" event). */
export interface TranslationPart {
  articleId: number;
  full: boolean;
  lang: TranslateLang;
  /** null for the title, else the paragraph's index. */
  index: number | null;
  total: number;
  text: string;
}

export interface ReaderPage {
  url: string;
  title: string;
  byline: string | null;
  siteName: string | null;
  excerpt: string | null;
  published: string | null;
  dir: string | null;
  html: string;
  blockedImages: number;
}

export interface Candidate {
  url: string;
  title: string;
  kind: string;
  /** The feed is on the local network: subscribing needs the user's consent. */
  local: boolean;
}

export interface ImportStats {
  /** New feeds added and new folders made. */
  feeds: number;
  folders: number;
  /** Subscribed feeds moved to the file's folders. */
  moved: number;
  /** Already subscribed, listed twice, or without a usable address. */
  skipped: number;
}

/** What an OPML import would do (opml::preview). */
export interface ImportPreview {
  fileName: string;
  inFile: number;
  newFeeds: number;
  subscribed: number;
  repeated: number;
  invalid: number;
  newTitles: string[];
  /** Subscribed feeds the file puts in another folder: how many, and the first few. */
  moveCount: number;
  moves: { title: string; from: string; to: string }[];
  newFoldersKeep: number;
  newFoldersFile: number;
}

/** Keep the folders of subscribed feeds, or move them to the file's folders. */
export type ImportLayout = 'keep' | 'file';

export interface AppInfo {
  version: string;
  dataDir: string;
  platform: string;
  /** Linux only: loopback server for podcast audio (WebKitGTK cannot play tidings:// media). */
  mediaBase?: string | null;
  /** The saved proxy could not be used at startup; Tidings is offline until it is fixed. */
  startupWarning?: string | null;
}

export interface Settings {
  refreshInterval: number;
  refreshOnStartup: boolean;
  retentionMode: 'all' | 'count' | 'days';
  retentionCount: number;
  retentionDays: number;
  loadImages: boolean;
  stripTracking: boolean;
  httpsOnly: boolean;
  proxy: string;
  userAgent: string;
  markReadDelay: number;
  linkTarget: 'tab' | 'browser';
  theme: 'system' | 'light' | 'dark' | 'sepia';
  accent: 'ember' | 'ocean' | 'forest' | 'violet' | 'rose' | 'graphite';
  readerFont: 'serif' | 'sans' | 'mono';
  readerSize: number;
  viewMode: 'widescreen' | 'normal' | 'combined';
  dockBadge: boolean;
  concurrency: number;
  /** External video player: empty = find mpv or IINA, else a program name or path. */
  videoPlayer: string;
  /** AI briefs: '' (off), 'ollama' or 'openai' (LM Studio, llama.cpp). Local servers only. */
  aiProvider: '' | 'ollama' | 'openai';
  aiEndpoint: string;
  aiModel: string;
  /** The model that translates articles; empty uses aiModel. */
  translateModel: string;
  /** The usual translation language: English (Chinese and Arabic articles) or Arabic (all others). */
  translateTo: TranslateLang;
}

/** A PeerTube video file chosen by src-tauri/src/peertube.rs. */
/** One line of the security check in Settings (updates.rs). */
export interface ComponentCheck {
  id: 'tidings' | 'yt-dlp' | 'webkit';
  name: string;
  version: string | null;
  detail: string | null;
  status: 'ok' | 'old' | 'update' | 'missing' | 'unknown' | 'info';
  message: string;
  command: string | null;
  link: string | null;
}

/** The audio of a video, found by yt-dlp, for the podcast player ("Listen"). */
export interface AudioStream {
  url: string;
  mime: string;
  duration: number | null;
}

export interface PeerTubeVideo {
  title: string;
  url: string;
  label: string;
  duration: number;
}

/** A Sepia Search result. `url` is the watch page; `thumbnail` goes through the image proxy. */
export interface PeerTubeHit {
  title: string;
  url: string;
  channel: string;
  host: string;
  duration: number;
  thumbnail: string | null;
  published: string | null;
}

/** The external player that plays video links (see src-tauri/src/video.rs). */
export interface VideoPlayer {
  name: string;
  path: string;
}

export interface FeedPatch {
  customTitle: string | null;
  url: string;
  folderId: number | null;
  updateInterval: number | null;
  retentionMode: string | null;
  retentionValue: number | null;
  fullText: boolean;
  paused: boolean;
  /** null for automatic. */
  typeOverride: FeedType | null;
  /** Allow this feed on the local network. Omitted: keep the current answer. */
  local?: boolean | null;
}

export interface ListQuery {
  scope: Scope;
  filter: Filter;
  query: string;
  oldestFirst: boolean;
  limit: number;
  offset: number;
  /** Published at or after, Unix seconds. */
  since?: number | null;
  /** Published before, Unix seconds. */
  until?: number | null;
  /** Only feeds of this type inside the scope (the type icons above the list). */
  feedType?: FeedType | null;
}

export interface FeedUpdated {
  feedId: number;
  newArticles: number;
  error: string | null;
}

export interface RefreshProgress {
  done: number;
  total: number;
}

/** A stored AI brief, without its body (brief.rs). */
export interface BriefInfo {
  id: number;
  createdAt: number;
  title: string;
  model: string;
}

export interface BriefSource {
  id: number;
  feedId: number;
  feed: string;
  title: string;
  published: number;
}

export interface Brief extends BriefInfo {
  /** The article filter it was written from. */
  query: ListQuery;
  sections: { title: string; points: { text: string; sources: number[] }[] }[];
  sources: BriefSource[];
  articlesUsed: number;
  articlesTotal: number;
  seconds: number;
}

export type Tab =
  | { id: string; kind: 'main' }
  | { id: string; kind: 'brief'; briefId: number | null; title: string; query?: ListQuery }
  | { id: string; kind: 'article'; articleId: number; feedId: number | null; title: string }
  | { id: string; kind: 'web'; url: string; title: string };

/** A feed or folder. Both share one order inside their parent. */
export type ItemRef = { kind: 'folder'; id: number } | { kind: 'feed'; id: number };

export type TreeChild = { kind: 'folder'; node: FolderNode } | { kind: 'feed'; feed: Feed };

export interface FolderNode {
  folder: Folder;
  children: TreeChild[];
}

export interface RootNode {
  children: TreeChild[];
}

export interface MenuItem {
  label?: string;
  icon?: string;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
  separator?: boolean;
  action?: () => void;
}

export type Dialog =
  | { kind: 'add-feed'; folderId: number | null; url?: string }
  | { kind: 'feed'; feedId: number }
  | { kind: 'folder'; folderId: number | null; parentId: number | null }
  | { kind: 'settings'; section?: string }
  | { kind: 'shortcuts' }
  | { kind: 'date-range' }
  | { kind: 'feed-health' }
  /** Name a saved search: a new one from the list as it is (`id` null), or rename one. */
  | { kind: 'save-search'; id: number | null; title: string }
  | { kind: 'restore'; summary: BackupSummary }
  | { kind: 'export-starred' }
  | { kind: 'import-opml'; preview: ImportPreview }
  | {
      kind: 'confirm';
      title: string;
      message: string;
      confirmLabel: string;
      danger?: boolean;
      onConfirm: () => void | Promise<void>;
    };
