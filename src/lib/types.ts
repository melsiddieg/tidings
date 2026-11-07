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
}

export interface ImportStats {
  feeds: number;
  folders: number;
  skipped: number;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  platform: string;
  /** Linux only: loopback server for podcast audio (WebKitGTK cannot play tidings:// media). */
  mediaBase?: string | null;
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

export type Tab =
  | { id: string; kind: 'main' }
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
  | {
      kind: 'confirm';
      title: string;
      message: string;
      confirmLabel: string;
      danger?: boolean;
      onConfirm: () => void | Promise<void>;
    };
