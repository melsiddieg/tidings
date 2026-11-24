// Application state and actions. Components read from `store` and call its methods.
import { api, errorText, on, setMediaBase } from './api';
import type {
  AppInfo,
  ArticleItem,
  BriefInfo,
  ComponentCheck,
  DateRange,
  Dialog,
  Feed,
  FeedType,
  FeedUpdated,
  Filter,
  FolderNode,
  ItemRef,
  LanguageCount,
  TreeChild,
  ListQuery,
  MenuItem,
  RefreshProgress,
  SavedSearch,
  Scope,
  SearchState,
  Settings,
  Tab,
  Tree,
  TreeSort,
  ImportLayout,
  TranslateLang,
  VideoPlayer,
} from './types';
import { buildTree, childRef, flattenFeeds, folderOptions, hostOf, isVideoLink, sameItem } from './util';
import {
  FEED_TYPES,
  SORT_KEYS,
  isAnyTime,
  rangeBounds,
  rangeLabel,
  sortLabel,
  typeInfo,
  languageName,
  validRange,
  validSort,
  type TypeInfo,
} from './filters';

const PAGE = 150;
const MAIN_TAB: Tab = { id: 'main', kind: 'main' };

export const DEFAULT_SETTINGS: Settings = {
  refreshInterval: 60,
  refreshOnStartup: true,
  retentionMode: 'count',
  retentionCount: 1000,
  retentionDays: 90,
  loadImages: true,
  stripTracking: true,
  httpsOnly: false,
  proxy: '',
  userAgent: '',
  markReadDelay: 0,
  linkTarget: 'tab',
  theme: 'system',
  accent: 'ember',
  readerFont: 'serif',
  readerSize: 18,
  viewMode: 'widescreen',
  dockBadge: true,
  concurrency: 6,
  videoPlayer: '',
  aiProvider: '',
  aiEndpoint: '',
  aiModel: '',
  translateModel: '',
  translateTo: 'en',
};

// UI preferences that are not worth a database round trip live in localStorage.
const local = {
  get<T>(key: string, fallback: T): T {
    try {
      const raw = localStorage.getItem(`tidings.${key}`);
      return raw ? (JSON.parse(raw) as T) : fallback;
    } catch {
      return fallback;
    }
  },
  set(key: string, value: unknown) {
    try {
      localStorage.setItem(`tidings.${key}`, JSON.stringify(value));
    } catch {
      /* storage full or disabled */
    }
  },
};

export interface Toast {
  id: number;
  text: string;
  kind: 'info' | 'error' | 'success';
}

export interface ContextMenu {
  x: number;
  y: number;
  items: MenuItem[];
}

export interface Layout {
  sidebar: number;
  list: number;
  listHeight: number;
}

class Store {
  ready = $state(false);
  tree = $state<Tree>({ folders: [], feeds: [], starred: 0 });
  settings = $state<Settings>({ ...DEFAULT_SETTINGS });
  info = $state<AppInfo | null>(null);
  /** Feeds and folders marked in the sidebar with ⌘/Ctrl- or Shift-click ("feed:1", "folder:2"). */
  marked = $state<string[]>([]);
  private markAnchor: string | null = null;
  /** The external video player, or null when none is found. */
  videoPlayer = $state<VideoPlayer | null>(null);
  /** AI briefs are set up: a local model server and a model (Settings → Local AI). */
  aiReady = $derived(this.settings.aiProvider !== '' && this.settings.aiModel.trim() !== '');
  /** Translations are set up: the same server, and a model for them or for briefs. */
  translateReady = $derived(this.settings.aiProvider !== '' && (this.settings.translateModel.trim() || this.settings.aiModel.trim()) !== '');
  /** Articles translated in this session, and into which language: they show
   *  translated when opened again. */
  translated = new Map<number, TranslateLang>();
  /** Brief tabs whose brief is being written, and the error of each that failed. */
  briefsRunning = $state<string[]>([]);
  briefErrors = $state<Record<string, string>>({});
  /** The security check in Settings (updates.rs), and when it last went online. */
  components = $state<ComponentCheck[] | null>(null);
  componentsCheckedOnline = $state<number | null>(null);
  checkingComponents = $state(false);
  /** Components that need an update. The Settings button shows a dot then. */
  componentWarnings = $derived((this.components ?? []).filter((c) => c.status === 'old' || c.status === 'update'));

  scope = $state<Scope>(local.get<Scope>('scope', { kind: 'all' }));
  filter = $state<Filter>(local.get<Filter>('filter', 'all'));
  query = $state('');
  oldestFirst = $state<boolean>(local.get('oldestFirst', false));
  dateRange = $state<DateRange>(validRange(local.get<unknown>('dateRange', null)));
  /** Only feeds of this type inside the scope (the type icons above the list). Resets
   *  when another feed or folder is selected. */
  typeFilter = $state<FeedType | null>(null);
  /** Only articles in this language (the language menu above the list), a code
   *  from language.rs. Resets with the type filter. */
  languageFilter = $state<string | null>(null);
  /** The languages of the articles in the scope (and type filter), with counts. */
  scopeLanguages = $state<LanguageCount[]>([]);
  /** Saved searches in the sidebar, and the unread articles each finds now. */
  searches = $state<SavedSearch[]>([]);
  searchCounts = $state<Record<number, number>>({});
  /** The saved search the list shows now, if it shows one exactly. */
  activeSearchId = $derived.by(() => {
    const now = JSON.stringify(this.searchState());
    return this.searches.find((s) => s.state === now)?.id ?? null;
  });

  articles = $state<ArticleItem[]>([]);
  hasMore = $state(false);
  listLoading = $state(false);
  /** Increases whenever the list is replaced, so views can reset scroll. */
  listVersion = $state(0);
  selectedId = $state<number | null>(null);
  /** Latest read/star state per article, shared by list, preview and tabs. */
  flags = $state<Record<number, { isRead?: boolean; isStarred?: boolean }>>({});

  tabs = $state<Tab[]>([MAIN_TAB]);
  activeTabId = $state('main');

  progress = $state<RefreshProgress | null>(null);
  lastRefresh = $state<number | null>(local.get<number | null>('lastRefresh', null));

  dialog = $state<Dialog | null>(null);
  menu = $state<ContextMenu | null>(null);
  toasts = $state<Toast[]>([]);
  layout = $state<Layout>(local.get<Layout>('layout', { sidebar: 264, list: 390, listHeight: 330 }));
  searchFocus = $state(0);
  /** Ticks every 30 s so relative times stay fresh. */
  now = $state(Date.now());
  /** Keyboard commands for the article view in the active tab. */
  command = $state<{ name: string; seq: number }>({ name: '', seq: 0 });

  /** How the sidebar orders subscriptions; kept on this computer. */
  treeSort = $state<TreeSort>(validSort(local.get<unknown>('treeSort', null)));
  root = $derived(buildTree(this.tree, this.treeSort));
  feedOrder = $derived(flattenFeeds(this.root));
  feedById = $derived(new Map(this.tree.feeds.map((f) => [f.id, f])));
  nodeById = $derived.by(() => {
    const map = new Map<number, FolderNode>();
    const walk = (children: TreeChild[]) => {
      for (const c of children) {
        if (c.kind === 'folder') {
          map.set(c.node.folder.id, c.node);
          walk(c.node.children);
        }
      }
    };
    walk(this.root.children);
    return map;
  });
  /** Favorite feeds and folders, in the order they were added. */
  favorites = $derived.by(() => {
    const items = [
      ...this.tree.folders.filter((f) => f.pinned > 0).map((f) => ({ ref: { kind: 'folder', id: f.id } as ItemRef, title: f.title, pinned: f.pinned })),
      ...this.tree.feeds.filter((f) => f.pinned > 0).map((f) => ({ ref: { kind: 'feed', id: f.id } as ItemRef, title: f.title, pinned: f.pinned })),
    ];
    return items.sort((a, b) => a.pinned - b.pinned || a.title.localeCompare(b.title));
  });
  folderById = $derived(new Map(this.tree.folders.map((f) => [f.id, f])));
  unreadTotal = $derived(this.tree.feeds.reduce((sum, f) => sum + f.unread, 0));
  /** Feeds whose name or address has every word of the search, in sidebar order. */
  feedMatches = $derived.by(() => {
    const words = this.query.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    return this.feedOrder.filter((f) => {
      const text = `${f.title} ${f.feedTitle} ${hostOf(f.url)} ${hostOf(f.siteUrl)}`.toLowerCase();
      return words.every((w) => text.includes(w));
    });
  });
  /** Feed types that have at least one feed, with their unread counts, in sidebar order. */
  typeSummary = $derived.by(() => {
    const rows: { info: TypeInfo; feeds: number; unread: number }[] = [];
    for (const info of FEED_TYPES) {
      const feeds = this.tree.feeds.filter((f) => f.feedType === info.value);
      if (feeds.length) rows.push({ info, feeds: feeds.length, unread: feeds.reduce((sum, f) => sum + f.unread, 0) });
    }
    return rows;
  });
  folderUnread = $derived.by(() => {
    const totals = new Map<number, number>();
    for (const feed of this.tree.feeds) {
      if (feed.folderId != null) totals.set(feed.folderId, (totals.get(feed.folderId) ?? 0) + feed.unread);
    }
    const depth = (id: number): number => {
      let d = 0;
      let cur = this.folderById.get(id);
      while (cur?.parentId != null && d < 64) {
        d++;
        cur = this.folderById.get(cur.parentId);
      }
      return d;
    };
    const folders = [...this.tree.folders].sort((a, b) => depth(b.id) - depth(a.id));
    for (const folder of folders) {
      if (folder.parentId != null) {
        totals.set(folder.parentId, (totals.get(folder.parentId) ?? 0) + (totals.get(folder.id) ?? 0));
      }
    }
    return totals;
  });
  activeTab = $derived(this.tabs.find((t) => t.id === this.activeTabId) ?? this.tabs[0]);
  scopeTitle = $derived.by(() => {
    const s = this.scope;
    if (s.kind === 'all') return 'All articles';
    if (s.kind === 'starred') return 'Starred';
    if (s.kind === 'feed') return this.feedById.get(s.id)?.title ?? 'Feed';
    if (s.kind === 'type') return typeInfo(s.type).label;
    return this.folderById.get(s.id)?.title ?? 'Folder';
  });
  /** The feed types inside the selected folder (everywhere for All and Starred), with
   *  unread counts. The list header shows them as icons when there are two or more. */
  scopeTypes = $derived.by(() => {
    const s = this.scope;
    let feeds: Feed[];
    if (s.kind === 'all' || s.kind === 'starred') {
      feeds = this.tree.feeds;
    } else if (s.kind === 'folder') {
      const inside = new Set(this.folderSubtree(s.id));
      feeds = this.tree.feeds.filter((f) => f.folderId != null && inside.has(f.folderId));
    } else {
      return [];
    }
    const rows = FEED_TYPES.map((info) => {
      const of = feeds.filter((f) => f.feedType === info.value);
      return { info, feeds: of.length, unread: of.reduce((sum, f) => sum + f.unread, 0) };
    }).filter((r) => r.feeds > 0);
    return rows.length > 1 ? rows : [];
  });
  scopeUnread = $derived.by(() => {
    const s = this.scope;
    if (this.languageFilter) {
      if (s.kind === 'starred') return 0;
      return this.scopeLanguages.find((l) => l.language === this.languageFilter)?.unread ?? 0;
    }
    const typed = this.typeFilter && this.scopeTypes.find((r) => r.info.value === this.typeFilter);
    if (typed) return s.kind === 'starred' ? 0 : typed.unread;
    if (s.kind === 'all') return this.unreadTotal;
    if (s.kind === 'starred') return 0;
    if (s.kind === 'feed') return this.feedById.get(s.id)?.unread ?? 0;
    if (s.kind === 'type') return this.typeSummary.find((r) => r.info.value === s.type)?.unread ?? 0;
    return this.folderUnread.get(s.id) ?? 0;
  });

  private listToken = 0;
  private languagesToken = 0;
  private readTimer: ReturnType<typeof setTimeout> | undefined;
  private updateTimer: ReturnType<typeof setTimeout> | undefined;
  private pendingFeeds = new Set<number>();
  private countsTimer: ReturnType<typeof setTimeout> | undefined;
  private toastId = 0;
  /** Feeds the user just added; their first refresh result is reported. */
  private watchFeeds = new Set<number>();

  // -------------------------------------------------------------------------
  // Startup
  // -------------------------------------------------------------------------

  async init() {
    try {
      const [settings, tree, info] = await Promise.all([api.settings(), api.tree(), api.info()]);
      this.settings = settings;
      this.tree = tree;
      this.info = info;
      setMediaBase(info.mediaBase);
      if (info.startupWarning) this.toast(info.startupWarning, 'error');
      // Set by `restoreBackup` before it reloaded the window.
      const notice = local.get<string | null>('notice', null);
      if (notice) {
        local.set('notice', null);
        this.toast(notice, 'success');
      }
    } catch (err) {
      this.error(err);
    }
    this.findVideoPlayer();
    this.checkComponents(true);
    document.documentElement.dataset.platform = this.info?.platform ?? 'web';
    this.applyAppearance();
    matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => this.applyAppearance());
    try {
      if (!this.scopeExists(this.scope)) this.scope = { kind: 'all' };
      this.loadSearches();
      this.restoreTabs();
      this.loadLanguages();
      await this.loadArticles(false);
      await on<FeedUpdated>('feed-updated', (e) => this.onFeedUpdated(e));
      await on<RefreshProgress>('refresh-progress', (p) => this.onProgress(p));
      await on<string>('menu', (id) => this.onMenu(id));
      // mpv can fail after play_video has returned, for example on a stream that does not open.
      await on<string>('video-failed', (text) => this.toast(text, 'error'));
    } catch (err) {
      this.error(err);
    } finally {
      setInterval(() => (this.now = Date.now()), 30_000);
      this.ready = true;
    }
  }

  send(name: string) {
    this.command = { name, seq: this.command.seq + 1 };
  }

  applyAppearance() {
    const s = this.settings;
    const root = document.documentElement;
    const dark = matchMedia('(prefers-color-scheme: dark)').matches;
    root.dataset.theme = s.theme === 'system' ? (dark ? 'dark' : 'light') : s.theme;
    root.dataset.accent = s.accent;
    root.dataset.readerFont = s.readerFont;
    root.style.setProperty('--reader-size', `${s.readerSize}px`);
  }

  private scopeExists(scope: Scope): boolean {
    if (scope.kind === 'feed') return this.feedById.has(scope.id);
    if (scope.kind === 'folder') return this.folderById.has(scope.id);
    if (scope.kind === 'type') return FEED_TYPES.some((t) => t.value === scope.type);
    return true;
  }

  // -------------------------------------------------------------------------
  // Toasts, menus, dialogs
  // -------------------------------------------------------------------------

  toast(text: string, kind: Toast['kind'] = 'info') {
    const id = ++this.toastId;
    this.toasts = [...this.toasts, { id, text, kind }].slice(-4);
    setTimeout(() => this.dismissToast(id), kind === 'error' ? 6500 : 3500);
  }

  dismissToast(id: number) {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  error(err: unknown) {
    this.toast(errorText(err), 'error');
  }

  openMenu(event: MouseEvent, items: MenuItem[]) {
    event.preventDefault();
    event.stopPropagation();
    this.menu = { x: event.clientX, y: event.clientY, items };
  }

  openMenuAt(el: HTMLElement, items: MenuItem[]) {
    const r = el.getBoundingClientRect();
    this.menu = { x: r.left, y: r.bottom + 4, items };
  }

  /** A second menu where the first one was opened ("Change type", "Sort"). */
  openMenuAtPoint(x: number, y: number, items: MenuItem[]) {
    this.menu = { x, y, items };
  }

  confirm(title: string, message: string, confirmLabel: string, onConfirm: () => void | Promise<void>, danger = true) {
    this.dialog = { kind: 'confirm', title, message, confirmLabel, danger, onConfirm };
  }

  saveLayout() {
    local.set('layout', this.layout);
  }

  // -------------------------------------------------------------------------
  // Tree and counts
  // -------------------------------------------------------------------------

  async reloadTree() {
    try {
      this.tree = await api.tree();
      if (!this.scopeExists(this.scope)) this.selectScope({ kind: 'all' });
      this.loadSearchCounts();
      this.loadLanguages();
    } catch (err) {
      this.error(err);
    }
  }

  refreshCounts() {
    clearTimeout(this.countsTimer);
    this.countsTimer = setTimeout(async () => {
      try {
        const counts = await api.counts();
        const map = new Map(counts.feeds);
        for (const feed of this.tree.feeds) feed.unread = map.get(feed.id) ?? 0;
        this.tree.starred = counts.starred;
        this.loadSearchCounts();
        this.loadLanguages();
      } catch {
        /* counts are cosmetic */
      }
    }, 120);
  }

  feedIcon(feedId: number): Feed | undefined {
    return this.feedById.get(feedId);
  }

  // -------------------------------------------------------------------------
  // Saved searches
  // -------------------------------------------------------------------------

  /** What a saved search keeps of the list: everything but the sort order. */
  searchState(): SearchState {
    return {
      scope: $state.snapshot(this.scope),
      filter: this.filter,
      query: this.query.trim(),
      dateRange: $state.snapshot(this.dateRange),
      typeFilter: this.typeFilter,
      // Only when set, so searches saved before there was a language menu still
      // match the list they show (activeSearchId).
      ...(this.languageFilter ? { language: this.languageFilter } : {}),
    };
  }

  /** A saved search's state, or null when it cannot be read or its feed or folder is gone. */
  parseSearch(search: SavedSearch): SearchState | null {
    try {
      const state = JSON.parse(search.state) as SearchState;
      return state && this.scopeExists(state.scope) ? state : null;
    } catch {
      return null;
    }
  }

  async loadSearches() {
    try {
      this.searches = await api.searches();
      await this.loadSearchCounts();
    } catch (err) {
      this.error(err);
    }
  }

  /** The unread articles each saved search finds now. */
  async loadSearchCounts() {
    const counts: Record<number, number> = {};
    for (const search of this.searches) {
      const state = this.parseSearch(search);
      if (!state) continue;
      const query: ListQuery = {
        scope: state.scope,
        filter: 'unread',
        query: state.query,
        oldestFirst: false,
        limit: 0,
        offset: 0,
        ...rangeBounds(validRange(state.dateRange)),
        feedType: state.typeFilter,
        language: state.language ?? null,
      };
      try {
        counts[search.id] = await api.countArticles(query);
      } catch {
        /* counts are cosmetic */
      }
    }
    this.searchCounts = counts;
  }

  /** Shows the list a saved search keeps. */
  openSearch(search: SavedSearch) {
    const state = this.parseSearch(search);
    if (!state) {
      this.toast('The feed or folder of this search was removed');
      return;
    }
    this.scope = state.scope;
    this.filter = state.filter;
    this.query = state.query;
    this.dateRange = validRange(state.dateRange);
    this.typeFilter = state.typeFilter;
    this.languageFilter = typeof state.language === 'string' ? state.language : null;
    local.set('scope', this.scope);
    local.set('filter', this.filter);
    local.set('dateRange', this.dateRange);
    this.selectedId = null;
    this.activeTabId = 'main';
    this.loadLanguages();
    this.loadArticles(false);
  }

  /** Saves the list as it is now (`id` null), or renames a saved search. */
  async nameSearch(id: number | null, title: string): Promise<boolean> {
    try {
      if (id == null) await api.saveSearch(title, JSON.stringify(this.searchState()));
      else await api.renameSearch(id, title);
      await this.loadSearches();
      return true;
    } catch (err) {
      this.error(err);
      return false;
    }
  }

  async deleteSearch(id: number) {
    try {
      await api.deleteSearch(id);
      await this.loadSearches();
    } catch (err) {
      this.error(err);
    }
  }

  // -------------------------------------------------------------------------
  // Article list
  // -------------------------------------------------------------------------

  listQuery(offset: number, limit = PAGE): ListQuery {
    return {
      scope: $state.snapshot(this.scope),
      filter: this.filter,
      query: this.query.trim(),
      oldestFirst: this.oldestFirst,
      limit,
      offset,
      ...rangeBounds(this.dateRange),
      feedType: this.typeFilter,
      language: this.languageFilter,
    };
  }

  /** The languages of the scope's articles, for the language menu. */
  async loadLanguages() {
    const token = ++this.languagesToken;
    try {
      const rows = await api.languageCounts({ ...this.listQuery(0, 0), language: null });
      if (token === this.languagesToken) this.scopeLanguages = rows;
    } catch {
      /* the menu is a convenience */
    }
  }

  async loadArticles(keepSelection = true) {
    const token = ++this.listToken;
    this.listLoading = true;
    try {
      const items = await api.listArticles(this.listQuery(0));
      if (token !== this.listToken) return;
      this.articles = items;
      this.hasMore = items.length === PAGE;
      this.listVersion++;
      if (!keepSelection) this.selectedId = null;
    } catch (err) {
      this.error(err);
    } finally {
      if (token === this.listToken) this.listLoading = false;
    }
  }

  async loadMore() {
    if (!this.hasMore || this.listLoading) return;
    const token = this.listToken;
    this.listLoading = true;
    try {
      const items = await api.listArticles(this.listQuery(this.articles.length));
      if (token !== this.listToken) return;
      const known = new Set(this.articles.map((a) => a.id));
      this.articles.push(...items.filter((a) => !known.has(a.id)));
      this.hasMore = items.length === PAGE;
    } catch (err) {
      this.error(err);
    } finally {
      this.listLoading = false;
    }
  }

  /** Adds newly fetched articles without removing the ones the user is looking at. */
  private async mergeNewArticles() {
    try {
      const fresh = await api.listArticles(this.listQuery(0));
      const known = new Set(this.articles.map((a) => a.id));
      const added = fresh.filter((a) => !known.has(a.id));
      if (!added.length) return;
      const merged = [...added, ...this.articles];
      merged.sort((a, b) =>
        this.oldestFirst ? a.published - b.published || a.id - b.id : b.published - a.published || b.id - a.id,
      );
      this.articles = merged;
    } catch {
      /* ignore background errors */
    }
  }

  selectScope(scope: Scope) {
    this.scope = scope;
    this.typeFilter = null;
    this.languageFilter = null;
    local.set('scope', scope);
    this.selectedId = null;
    this.activeTabId = 'main';
    this.loadLanguages();
    this.loadArticles(false);
  }

  setFilter(filter: Filter) {
    this.filter = filter;
    local.set('filter', filter);
    this.loadArticles();
  }

  /** Shows only one feed type inside the scope, or all types (null). */
  setTypeFilter(type: FeedType | null) {
    this.typeFilter = type;
    this.loadLanguages();
    this.loadArticles();
  }

  /** Shows only the articles in one language, or in all (null). */
  setLanguageFilter(language: string | null) {
    this.languageFilter = language;
    this.loadArticles();
  }

  setQuery(query: string) {
    this.query = query;
    this.loadArticles();
  }

  setDateRange(range: DateRange) {
    this.dateRange = range;
    local.set('dateRange', range);
    this.loadArticles();
  }

  /** True when the list is narrowed to some dates. */
  get dateFiltered(): boolean {
    return !isAnyTime(this.dateRange);
  }

  setOldestFirst(value: boolean) {
    this.oldestFirst = value;
    local.set('oldestFirst', value);
    this.loadArticles();
  }

  isRead(a: { id: number; isRead: boolean }): boolean {
    return this.flags[a.id]?.isRead ?? a.isRead;
  }

  isStarred(a: { id: number; isStarred: boolean }): boolean {
    return this.flags[a.id]?.isStarred ?? a.isStarred;
  }

  selectArticle(id: number) {
    this.selectedId = id;
    clearTimeout(this.readTimer);
    const item = this.articles.find((a) => a.id === id);
    if (!item || this.isRead(item)) return;
    const delay = this.settings.markReadDelay;
    const mark = () => {
      if (this.selectedId === id) this.markRead([id], true);
    };
    if (delay <= 0) mark();
    else this.readTimer = setTimeout(mark, delay * 1000);
  }

  async markRead(ids: number[], read: boolean) {
    const changed: number[] = [];
    for (const id of ids) {
      const item = this.articles.find((a) => a.id === id);
      const current = this.flags[id]?.isRead ?? item?.isRead;
      if (current === read) continue;
      changed.push(id);
      this.flags[id] = { ...this.flags[id], isRead: read };
      if (item) {
        item.isRead = read;
        if (read) item.isNew = false;
        const feed = this.feedById.get(item.feedId);
        if (feed) feed.unread = Math.max(0, feed.unread + (read ? -1 : 1));
      }
    }
    if (!changed.length) return;
    try {
      await api.setRead(changed, read);
    } catch (err) {
      this.error(err);
    }
    this.refreshCounts();
  }

  async setStarred(id: number, starred: boolean) {
    this.flags[id] = { ...this.flags[id], isStarred: starred };
    const item = this.articles.find((a) => a.id === id);
    if (item) item.isStarred = starred;
    this.tree.starred = Math.max(0, this.tree.starred + (starred ? 1 : -1));
    try {
      await api.setStarred(id, starred);
    } catch (err) {
      this.error(err);
    }
    this.refreshCounts();
  }

  /**
   * Without a scope: everything in the list as shown, including its date range
   * ("Older than 30 days", then mark all as read). With a scope (sidebar menus): all of it.
   */
  async markAllRead(scope?: Scope) {
    const current = scope === undefined;
    const target = scope ?? this.scope;
    const { since, until } = current ? rangeBounds(this.dateRange) : { since: null, until: null };
    const feedType = current ? this.typeFilter : null;
    const language = current ? this.languageFilter : null;
    try {
      const n = await api.markAllRead($state.snapshot(target), since, until, feedType, language);
      if (JSON.stringify(target) === JSON.stringify(this.scope)) {
        for (const a of this.articles) {
          a.isRead = true;
          a.isNew = false;
          this.flags[a.id] = { ...this.flags[a.id], isRead: true };
        }
      } else {
        await this.loadArticles();
      }
      this.refreshCounts();
      this.toast(n === 1 ? 'Marked 1 article as read' : `Marked ${n} articles as read`, 'success');
    } catch (err) {
      this.error(err);
    }
  }

  deleteArticle(id: number) {
    this.confirm('Delete article?', 'It will be removed and will not come back when the feed refreshes.', 'Delete', async () => {
      try {
        await api.deleteArticles([id]);
        const index = this.articles.findIndex((a) => a.id === id);
        if (index >= 0) this.articles.splice(index, 1);
        if (this.selectedId === id) this.selectedId = this.articles[index]?.id ?? this.articles[index - 1]?.id ?? null;
        this.tabs = this.tabs.filter((t) => !(t.kind === 'article' && t.articleId === id));
        if (!this.tabs.some((t) => t.id === this.activeTabId)) this.activeTabId = 'main';
        this.persistTabs();
        this.refreshCounts();
      } catch (err) {
        this.error(err);
      }
    });
  }

  /** Moves the selection by `step` articles in the list. */
  moveSelection(step: number) {
    if (!this.articles.length) return;
    const index = this.articles.findIndex((a) => a.id === this.selectedId);
    const next = index < 0 ? (step > 0 ? 0 : this.articles.length - 1) : index + step;
    const item = this.articles[Math.max(0, Math.min(this.articles.length - 1, next))];
    if (item) this.selectArticle(item.id);
    if (next >= this.articles.length - 5) this.loadMore();
  }

  /** Next unread article in the list; continues into the next feed with unread articles. */
  async nextUnread(step: 1 | -1 = 1) {
    const index = this.articles.findIndex((a) => a.id === this.selectedId);
    const pool = step > 0 ? this.articles.slice(index + 1) : this.articles.slice(0, Math.max(0, index)).reverse();
    const hit = pool.find((a) => !this.isRead(a));
    if (hit) return this.selectArticle(hit.id);
    if (step > 0 && this.hasMore) {
      await this.loadMore();
      const more = this.articles.slice(index + 1).find((a) => !this.isRead(a));
      if (more) return this.selectArticle(more.id);
    }
    if (this.scope.kind === 'feed' || this.scope.kind === 'folder') {
      const order = this.feedOrder;
      const current = this.scope.kind === 'feed' ? order.findIndex((f) => f.id === (this.scope as { id: number }).id) : -1;
      for (let i = 1; i <= order.length; i++) {
        const feed = order[(current + i * step + order.length * 2) % order.length];
        if (feed && feed.unread > 0 && feed.id !== (this.scope as { id: number }).id) {
          this.selectScope({ kind: 'feed', id: feed.id });
          await this.loadArticles(false);
          const first = this.articles.find((a) => !this.isRead(a));
          if (first) this.selectArticle(first.id);
          return;
        }
      }
    }
    this.toast('No more unread articles');
  }

  // -------------------------------------------------------------------------
  // Tabs
  // -------------------------------------------------------------------------

  private restoreTabs() {
    // A brief that was still being written when the app closed is gone.
    const saved = local.get<Tab[]>('tabs', []).filter((t) => t.kind !== 'main' && !(t.kind === 'brief' && t.briefId == null));
    this.tabs = [MAIN_TAB, ...saved];
    const active = local.get<string>('activeTab', 'main');
    this.activeTabId = this.tabs.some((t) => t.id === active) ? active : 'main';
  }

  persistTabs() {
    local.set('tabs', this.tabs.filter((t) => t.kind !== 'main'));
    local.set('activeTab', this.activeTabId);
  }

  private insertTab(tab: Tab, background: boolean) {
    const at = this.tabs.findIndex((t) => t.id === this.activeTabId);
    const tabs = [...this.tabs];
    tabs.splice(at + 1, 0, tab);
    this.tabs = tabs;
    if (!background) this.activeTabId = tab.id;
    this.persistTabs();
  }

  /** `known` names an article that may not be in the list, such as a brief's source. */
  openArticleTab(id: number, background = false, known?: { title: string; feedId: number }) {
    const existing = this.tabs.find((t) => t.kind === 'article' && t.articleId === id);
    if (existing) {
      if (!background) this.activateTab(existing.id);
      return;
    }
    const item = this.articles.find((a) => a.id === id);
    this.insertTab(
      {
        id: `a${id}-${Date.now().toString(36)}`,
        kind: 'article',
        articleId: id,
        feedId: item?.feedId ?? known?.feedId ?? null,
        title: item?.title ?? known?.title ?? 'Article',
      },
      background,
    );
    if (item && !this.isRead(item)) this.markRead([id], true);
  }

  openWebTab(url: string, background = false, title?: string) {
    const existing = this.tabs.find((t) => t.kind === 'web' && t.url === url);
    if (existing) {
      if (!background) this.activateTab(existing.id);
      return;
    }
    this.insertTab({ id: `w${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`, kind: 'web', url, title: title ?? url }, background);
  }

  /** Handles a click on a link inside article content. */
  openLink(url: string, mode: 'default' | 'background' | 'browser' = 'default') {
    if (mode === 'default' && this.videoPlayer && isVideoLink(url)) {
      this.watchVideo(url);
      return;
    }
    if (url.startsWith('mailto:') || mode === 'browser' || (mode === 'default' && this.settings.linkTarget === 'browser')) {
      this.openExternal(url);
      return;
    }
    this.openWebTab(url, mode === 'background');
  }

  /**
   * The offline security check: versions on this computer only, no network. At
   * startup (`notify`), a finding is announced once a week at most.
   */
  async checkComponents(notify = false) {
    try {
      // At startup no program is started: yt-dlp's version is the one seen at the
      // last check from Settings.
      this.components = await api.checkComponents(!notify);
    } catch {
      return;
    }
    const warnings = this.componentWarnings;
    if (!notify || warnings.length === 0) return;
    const key = warnings.map((c) => `${c.id}:${c.version}`).join(',');
    const last = local.get<{ key: string; at: number } | null>('securityNotice', null);
    if (last && last.key === key && Date.now() - last.at < 7 * 86_400_000) return;
    local.set('securityNotice', { key, at: Date.now() });
    const names = warnings.map((c) => c.name).join(' and ');
    this.toast(`${names} ${warnings.length > 1 ? 'need' : 'needs'} an update. See Settings → Security.`, 'info');
  }

  /** The online security check: only on the user's click ("Check now"). */
  async checkComponentsOnline() {
    this.checkingComponents = true;
    try {
      this.components = await api.checkComponentsOnline();
      this.componentsCheckedOnline = Date.now();
    } catch (err) {
      this.error(err);
    } finally {
      this.checkingComponents = false;
    }
  }

  /** Looks for the external video player again (the user may have installed one). */
  async findVideoPlayer() {
    try {
      this.videoPlayer = await api.videoPlayer();
    } catch {
      this.videoPlayer = null;
    }
  }

  /** Plays a video link in mpv or IINA, without the site's own web player. */
  async watchVideo(url: string | null | undefined) {
    if (!url) return;
    if (this.videoPlayer) this.toast(`Opening in ${this.videoPlayer.name}…`);
    try {
      await api.playVideo(url);
    } catch (err) {
      this.error(err);
    } finally {
      this.findVideoPlayer();
    }
  }

  async openExternal(url: string | null | undefined) {
    if (!url) return;
    try {
      await api.openExternal(url);
    } catch (err) {
      this.error(err);
    }
  }

  setTabTitle(id: string, title: string) {
    const tab = this.tabs.find((t) => t.id === id);
    if (tab && tab.kind !== 'main' && title && tab.title !== title) {
      tab.title = title;
      this.persistTabs();
    }
  }

  activateTab(id: string) {
    this.activeTabId = id;
    this.persistTabs();
  }

  // -------------------------------------------------------------------------
  // AI briefs (brief.rs): written by a local model, shown in tabs
  // -------------------------------------------------------------------------

  /** A name for what the list shows: "Canadian News · Unread · Last 24 hours". */
  briefTitle(): string {
    const parts = [this.scopeTitle];
    if (this.typeFilter) parts.push(typeInfo(this.typeFilter).label);
    if (this.languageFilter) parts.push(languageName(this.languageFilter));
    if (this.filter !== 'all') parts.push(this.filter[0].toUpperCase() + this.filter.slice(1));
    if (!isAnyTime(this.dateRange)) parts.push(rangeLabel(this.dateRange));
    if (this.query.trim()) parts.push(`“${this.query.trim()}”`);
    return parts.join(' · ');
  }

  /** Writes a brief of what the list shows now, in a new tab. */
  makeBrief() {
    if (!this.aiReady) {
      this.dialog = { kind: 'settings', section: 'ai' };
      return;
    }
    const tab: Tab = { id: `b${Date.now().toString(36)}`, kind: 'brief', briefId: null, title: this.briefTitle(), query: this.listQuery(0) };
    this.insertTab(tab, false);
    this.runBrief(tab.id);
  }

  /** Writes the brief of a brief tab (again). */
  async runBrief(tabId: string) {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (!tab || tab.kind !== 'brief' || !tab.query || this.briefsRunning.includes(tabId)) return;
    this.briefsRunning = [...this.briefsRunning, tabId];
    const { [tabId]: _old, ...errors } = this.briefErrors;
    this.briefErrors = errors;
    try {
      const brief = await api.makeBrief($state.snapshot(tab.query), tab.title);
      this.tabs = this.tabs.map((t) => (t.id === tabId && t.kind === 'brief' ? { ...t, briefId: brief.id } : t));
      this.persistTabs();
    } catch (err) {
      this.briefErrors = { ...this.briefErrors, [tabId]: errorText(err) };
    } finally {
      this.briefsRunning = this.briefsRunning.filter((id) => id !== tabId);
    }
  }

  /** Writes a stored brief again, from the same articles, in its tab. */
  rewriteBrief(tabId: string, query: ListQuery) {
    this.tabs = this.tabs.map((t) => (t.id === tabId && t.kind === 'brief' ? { ...t, briefId: null, query } : t));
    this.runBrief(tabId);
  }

  /** Opens a stored brief, or its tab if it is open. */
  openBrief(info: BriefInfo) {
    const existing = this.tabs.find((t) => t.kind === 'brief' && t.briefId === info.id);
    if (existing) return this.activateTab(existing.id);
    this.insertTab({ id: `b${info.id}-${Date.now().toString(36)}`, kind: 'brief', briefId: info.id, title: info.title }, false);
  }

  async deleteBrief(tabId: string, id: number) {
    try {
      await api.deleteBrief(id);
      this.closeTab(tabId);
    } catch (err) {
      this.error(err);
    }
  }

  closeTab(id: string) {
    if (id === 'main') return;
    const index = this.tabs.findIndex((t) => t.id === id);
    if (index < 0) return;
    const tabs = this.tabs.filter((t) => t.id !== id);
    if (this.activeTabId === id) this.activeTabId = (tabs[index] ?? tabs[index - 1] ?? MAIN_TAB).id;
    this.tabs = tabs;
    this.persistTabs();
  }

  closeOtherTabs(id: string) {
    this.tabs = this.tabs.filter((t) => t.kind === 'main' || t.id === id);
    if (!this.tabs.some((t) => t.id === this.activeTabId)) this.activeTabId = id;
    this.persistTabs();
  }

  cycleTab(step: number) {
    const index = this.tabs.findIndex((t) => t.id === this.activeTabId);
    const next = this.tabs[(index + step + this.tabs.length) % this.tabs.length];
    if (next) this.activateTab(next.id);
  }

  // -------------------------------------------------------------------------
  // Refresh
  // -------------------------------------------------------------------------

  async refresh(target: { feedIds?: number[]; folderId?: number } = {}) {
    try {
      const n = await api.refresh(target);
      if (n === 0) this.toast('Nothing to refresh');
    } catch (err) {
      this.error(err);
    }
  }

  refreshScope() {
    const s = this.scope;
    if (s.kind === 'feed') return this.refresh({ feedIds: [s.id] });
    if (s.kind === 'folder') return this.refresh({ folderId: s.id });
    if (s.kind === 'type') return this.refresh({ feedIds: this.feedsOfType(s.type).map((f) => f.id) });
    return this.refresh();
  }

  feedsOfType(type: FeedType): Feed[] {
    return this.tree.feeds.filter((f) => f.feedType === type);
  }

  watchFeed(id: number) {
    this.watchFeeds.add(id);
  }

  private scopeIncludes(feedId: number): boolean {
    const s = this.scope;
    if (s.kind === 'all') return true;
    if (s.kind === 'starred') return false;
    if (s.kind === 'feed') return s.id === feedId;
    if (s.kind === 'type') return this.feedById.get(feedId)?.feedType === s.type;
    let folder = this.feedById.get(feedId)?.folderId ?? null;
    for (let guard = 0; folder != null && guard < 64; guard++) {
      if (folder === s.id) return true;
      folder = this.folderById.get(folder)?.parentId ?? null;
    }
    return false;
  }

  private onFeedUpdated(e: FeedUpdated) {
    if (this.watchFeeds.delete(e.feedId)) {
      const feed = this.feedById.get(e.feedId);
      if (e.error) this.toast(`${feed?.title ?? 'Feed'}: ${e.error}`, 'error');
    }
    if (e.newArticles > 0 || e.error) this.pendingFeeds.add(e.feedId);
    clearTimeout(this.updateTimer);
    this.updateTimer = setTimeout(async () => {
      const touched = [...this.pendingFeeds];
      this.pendingFeeds.clear();
      await this.reloadTree();
      if (touched.some((id) => this.scopeIncludes(id))) {
        if (this.articles.length === 0) await this.loadArticles();
        else await this.mergeNewArticles();
      }
    }, 450);
  }

  private onProgress(p: RefreshProgress) {
    if (p.total > 0 && p.done < p.total) {
      this.progress = p;
    } else {
      this.progress = null;
      this.lastRefresh = Date.now();
      local.set('lastRefresh', this.lastRefresh);
    }
  }

  // -------------------------------------------------------------------------
  // Subscriptions
  // -------------------------------------------------------------------------

  /** Whether any folder is open: the sidebar offers "Collapse all" then, else "Expand all". */
  get anyFolderOpen(): boolean {
    return this.tree.folders.some((f) => f.expanded);
  }

  /** Opens or closes the folders `ids`, or every folder when `ids` is omitted. */
  async setExpanded(expanded: boolean, ids?: number[]) {
    const targets = ids ? ids.map((id) => this.folderById.get(id)) : this.tree.folders;
    for (const f of targets) if (f) f.expanded = expanded;
    try {
      await api.setFoldersExpanded(ids ?? null, expanded);
    } catch (err) {
      this.error(err);
    }
  }

  /** A folder and every folder inside it. */
  folderSubtree(id: number): number[] {
    const out = [id];
    for (let i = 0; i < out.length; i++) {
      for (const f of this.tree.folders) if (f.parentId === out[i]) out.push(f.id);
    }
    return out;
  }

  /** Feeds and folders in the order the sidebar shows them (inside open folders only). */
  private visibleItems(): string[] {
    const out: string[] = [];
    const walk = (children: TreeChild[]) => {
      for (const c of children) {
        if (c.kind === 'feed') {
          out.push(`feed:${c.feed.id}`);
        } else {
          out.push(`folder:${c.node.folder.id}`);
          if (c.node.folder.expanded) walk(c.node.children);
        }
      }
    };
    walk(this.root.children);
    return out;
  }

  /** The feed or folder shown in the article list, if that is what is selected. */
  private scopeKey(): string | null {
    const s = this.scope;
    return s.kind === 'feed' || s.kind === 'folder' ? `${s.kind}:${s.id}` : null;
  }

  /** A click on a feed or folder in the sidebar. ⌘/Ctrl adds it to the marked items or
   *  removes it, Shift marks everything between it and the last click, and a plain
   *  click selects it and clears the marks. */
  clickItem(ref: ItemRef, e: MouseEvent) {
    const key = `${ref.kind}:${ref.id}`;
    if (e.metaKey || e.ctrlKey) {
      // Like Finder: the item already selected joins the first ⌘-click.
      const current = this.scopeKey();
      if (this.marked.length === 0 && current && current !== key) this.marked = [current];
      this.marked = this.marked.includes(key) ? this.marked.filter((k) => k !== key) : [...this.marked, key];
      this.markAnchor = key;
      return;
    }
    if (e.shiftKey) {
      const keys = this.visibleItems();
      const to = keys.indexOf(key);
      const from = keys.indexOf(this.markAnchor ?? this.scopeKey() ?? key);
      const [a, b] = from < 0 ? [to, to] : from <= to ? [from, to] : [to, from];
      // Like Finder, the range adds to what is already marked.
      this.marked = [...new Set([...this.marked, ...keys.slice(a, b + 1)])];
      return;
    }
    this.marked = [];
    this.markAnchor = key;
    this.selectScope(ref.kind === 'feed' ? { kind: 'feed', id: ref.id } : { kind: 'folder', id: ref.id });
  }

  clearMarks() {
    this.marked = [];
    this.markAnchor = null;
  }

  /** The marked items that still exist. */
  private markedItems(): ItemRef[] {
    return this.marked
      .map((k) => {
        const [kind, id] = k.split(':');
        return { kind, id: Number(id) } as ItemRef;
      })
      .filter((r) => (r.kind === 'feed' ? this.feedById.has(r.id) : this.folderById.has(r.id)));
  }

  /** Deletes the marked feeds and folders after one confirmation. */
  deleteMarked() {
    const items = this.markedItems();
    if (items.length === 0) return;
    const folders = items.filter((r) => r.kind === 'folder').length;
    const feeds = items.length - folders;
    const count = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
    const what = [folders && count(folders, 'folder'), feeds && count(feeds, 'feed')].filter(Boolean).join(' and ');
    this.confirm(
      `Delete ${what}?`,
      (folders ? 'Folders go with every subfolder and feed inside them. ' : '') +
        'All their articles, including starred ones, will be deleted from this computer.',
      'Delete',
      async () => {
        try {
          const n = await api.deleteItems(items);
          this.clearMarks();
          await this.reloadTree();
          if (!this.scopeExists(this.scope)) this.selectScope({ kind: 'all' });
          else await this.loadArticles();
          this.toast(n === 1 ? 'Deleted 1 feed' : `Deleted ${n} feeds`, 'success');
        } catch (err) {
          this.error(err);
        }
      },
    );
  }

  /** Removes every feed and folder, to start over. Settings stay. */
  deleteAllSubscriptions() {
    const feeds = this.tree.feeds.length;
    const folders = this.tree.folders.length;
    if (feeds + folders === 0) return this.toast('There are no subscriptions to remove');
    const count = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
    this.confirm(
      'Remove all subscriptions?',
      `All ${count(feeds, 'feed')} and ${count(folders, 'folder')} will be deleted, with every article, including starred ones. ` +
        'Settings stay. Export OPML first if you may want them back.',
      'Remove all',
      async () => {
        try {
          await api.deleteAllSubscriptions();
          this.clearMarks();
          await this.reloadTree();
          this.selectScope({ kind: 'all' });
          this.toast('All subscriptions removed', 'success');
        } catch (err) {
          this.error(err);
        }
      },
    );
  }

  /** The context menu for a right-click on one of several marked items. */
  markedMenu(e: MouseEvent) {
    const n = this.marked.length;
    const feeds = this.marked
      .filter((key) => key.startsWith('feed:'))
      .map((key) => this.feedById.get(Number(key.slice(5))))
      .filter((f): f is Feed => !!f);
    const { clientX: x, clientY: y } = e;
    this.openMenu(e, [
      ...(feeds.length
        ? [
            {
              label: feeds.length === 1 ? 'Change type of 1 feed' : `Change type of ${feeds.length} feeds`,
              icon: 'filter',
              action: () => this.openMenuAtPoint(x, y, this.typeMenuItems(feeds)),
            },
            { separator: true },
          ]
        : []),
      { label: `Delete ${n} items…`, icon: 'trash', danger: true, action: () => this.deleteMarked() },
      { separator: true },
      { label: 'Clear selection', icon: 'x', action: () => this.clearMarks() },
    ]);
  }

  async toggleFolder(id: number) {
    const folder = this.folderById.get(id);
    if (!folder) return;
    folder.expanded = !folder.expanded;
    try {
      await api.setFolderExpanded(id, folder.expanded);
    } catch (err) {
      this.error(err);
    }
  }

  /** "Technology / Linux" for a folder, "" for the top level. */
  folderPath(id: number | null): string {
    const names: string[] = [];
    let f = id == null ? undefined : this.folderById.get(id);
    for (let depth = 0; f && depth < 64; depth++) {
      names.unshift(f.title);
      f = f.parentId == null ? undefined : this.folderById.get(f.parentId);
    }
    return names.join(' / ');
  }

  /** Folders to move a feed to, in sidebar order; a check marks where it is. */
  folderMenuItems(feed: Feed): MenuItem[] {
    const folders = folderOptions(this.root);
    return [
      { label: 'No folder', checked: feed.folderId == null, action: () => this.moveFeedTo(feed, null) },
      ...(folders.length ? [{ separator: true }] : []),
      ...folders.map((f) => ({
        // Em spaces indent like the tree; the menu does not collapse them.
        label: `${'\u2003'.repeat(f.depth)}${f.title}`,
        checked: feed.folderId === f.id,
        action: () => this.moveFeedTo(feed, f.id),
      })),
    ];
  }

  async moveFeedTo(feed: Feed, folderId: number | null) {
    if (feed.folderId === folderId) return;
    if (!(await this.moveItem({ kind: 'feed', id: feed.id }, folderId, null))) return;
    if (folderId != null) this.revealFolder(folderId);
    this.toast(`Moved “${feed.title}” to ${folderId == null ? 'the top level' : this.folderPath(folderId)}`, 'success');
  }

  /** Leaves the search and shows a feed, with its folder open in the sidebar. */
  showFeed(feed: Feed) {
    this.query = '';
    if (feed.folderId != null) this.revealFolder(feed.folderId);
    this.selectScope({ kind: 'feed', id: feed.id });
  }

  /** Opens a folder and the folders around it, so a feed added there shows. */
  revealFolder(id: number) {
    const closed: number[] = [];
    let f = this.folderById.get(id);
    for (let depth = 0; f && depth < 64; depth++) {
      if (!f.expanded) closed.push(f.id);
      f = f.parentId == null ? undefined : this.folderById.get(f.parentId);
    }
    if (closed.length) this.setExpanded(true, closed);
  }

  /** The Add Feed dialog. Without a folder it uses the selected folder, or the
   *  folder of the selected feed. */
  addFeed(folderId?: number | null) {
    let target = folderId ?? null;
    if (folderId === undefined) {
      const s = this.scope;
      if (s.kind === 'folder') target = s.id;
      else if (s.kind === 'feed') target = this.feedById.get(s.id)?.folderId ?? null;
    }
    this.dialog = { kind: 'add-feed', folderId: target };
  }

  setTreeSort(patch: Partial<TreeSort>) {
    this.treeSort = { ...this.treeSort, ...patch };
    local.set('treeSort', this.treeSort);
  }

  /** The sort menu: what to sort by, which way, and whether feeds are sorted too. */
  sortMenuItems(): MenuItem[] {
    const sort = this.treeSort;
    const manual = sort.key === 'manual';
    const key = SORT_KEYS.find((k) => k.key === sort.key) ?? SORT_KEYS[0];
    return [
      ...SORT_KEYS.map((k) => ({ label: k.label, checked: sort.key === k.key, action: () => this.setTreeSort({ key: k.key }) })),
      { separator: true },
      { label: manual ? 'Ascending' : `Ascending (${key.up})`, checked: !manual && !sort.descending, disabled: manual, action: () => this.setTreeSort({ descending: false }) },
      { label: manual ? 'Descending' : `Descending (${key.down})`, checked: !manual && sort.descending, disabled: manual, action: () => this.setTreeSort({ descending: true }) },
      { separator: true },
      { label: 'Sort feeds too', checked: !manual && sort.feeds, disabled: manual, action: () => this.setTreeSort({ feeds: !sort.feeds }) },
    ];
  }

  /** Types to choose from for these feeds. A check marks the choice they all share. */
  typeMenuItems(feeds: Feed[]): MenuItem[] {
    const all = (type: FeedType | null) => feeds.every((f) => f.typeOverride === type);
    const detected = feeds.length === 1 ? ` (${typeInfo(feeds[0].detectedType).one})` : '';
    return [
      { label: `Automatic${detected}`, checked: all(null), action: () => this.setFeedType(feeds, null) },
      { separator: true },
      ...FEED_TYPES.map((t) => ({ label: t.one, checked: all(t.value), action: () => this.setFeedType(feeds, t.value) })),
    ];
  }

  /** Sets the type of feeds by hand; `null` goes back to the detected type. */
  async setFeedType(feeds: Feed[], type: FeedType | null) {
    if (!feeds.length) return;
    try {
      await api.setFeedType(
        feeds.map((f) => f.id),
        type,
      );
      await this.reloadTree();
      // The list may show a type scope or a type filter that these feeds left or joined.
      await this.loadArticles();
      const what = feeds.length === 1 ? `“${feeds[0].title}”` : `${feeds.length} feeds`;
      if (type) this.toast(`${what} now ${feeds.length === 1 ? 'shows' : 'show'} under ${typeInfo(type).label}`, 'success');
      else this.toast(`${what} ${feeds.length === 1 ? 'uses its' : 'use their'} detected type again`, 'success');
    } catch (err) {
      this.error(err);
    }
  }

  deleteFeed(feed: Feed) {
    this.confirm(
      `Unsubscribe from “${feed.title}”?`,
      'The feed and all of its articles, including starred ones, will be deleted from this computer.',
      'Unsubscribe',
      async () => {
        try {
          await api.deleteFeed(feed.id);
          if (this.scope.kind === 'feed' && this.scope.id === feed.id) this.selectScope({ kind: 'all' });
          await this.reloadTree();
          await this.loadArticles();
        } catch (err) {
          this.error(err);
        }
      },
    );
  }

  deleteFolder(id: number) {
    const folder = this.folderById.get(id);
    if (!folder) return;
    this.confirm(
      `Delete folder “${folder.title}”?`,
      'Every subfolder and feed inside it will be deleted, together with their articles.',
      'Delete folder',
      async () => {
        try {
          await api.deleteFolder(id);
          await this.reloadTree();
          if (!this.scopeExists(this.scope)) this.selectScope({ kind: 'all' });
          else await this.loadArticles();
        } catch (err) {
          this.error(err);
        }
      },
    );
  }

  /** Moves a feed or folder into `parentId` (null = top level), before `before` or at the end. */
  async moveItem(item: ItemRef, parentId: number | null, before: ItemRef | null): Promise<boolean> {
    if (sameItem(item, before)) return false;
    // In a sorted list a new place in the same folder would not show. Moving into
    // another folder still works.
    const sorted = this.treeSort.key !== 'manual' && (item.kind === 'folder' || this.treeSort.feeds);
    const parentNow = item.kind === 'feed' ? this.feedById.get(item.id)?.folderId : this.folderById.get(item.id)?.parentId;
    if (sorted && (parentNow ?? null) === parentId) {
      this.toast(`The list is sorted by ${sortLabel(this.treeSort).toLowerCase()}. To arrange it by hand, choose Manual order in the sort menu.`);
      return false;
    }
    try {
      if (item.kind === 'feed') await api.moveFeed(item.id, parentId, before);
      else await api.moveFolder(item.id, parentId, before);
      await this.reloadTree();
      return true;
    } catch (err) {
      this.error(err);
      return false;
    }
  }

  /** The item that follows `target` in its parent, once `moving` is taken out. */
  itemAfter(parentId: number | null, target: ItemRef, moving: ItemRef): ItemRef | null {
    const siblings = (parentId == null ? this.root.children : (this.nodeById.get(parentId)?.children ?? []))
      .map(childRef)
      .filter((r) => !sameItem(r, moving));
    const index = siblings.findIndex((r) => sameItem(r, target));
    return index >= 0 ? (siblings[index + 1] ?? null) : null;
  }

  isPinned(item: ItemRef): boolean {
    const pinned = item.kind === 'feed' ? this.feedById.get(item.id)?.pinned : this.folderById.get(item.id)?.pinned;
    return (pinned ?? 0) > 0;
  }

  async setPinned(item: ItemRef, pinned: boolean) {
    const target = item.kind === 'feed' ? this.feedById.get(item.id) : this.folderById.get(item.id);
    if (target) target.pinned = pinned ? Math.floor(Date.now() / 1000) : 0;
    try {
      await api.setPinned(item, pinned);
    } catch (err) {
      this.error(err);
      await this.reloadTree();
    }
  }

  /** Picks an OPML file and shows what importing it would do. */
  async importOpml() {
    try {
      const preview = await api.previewOpml();
      if (!preview) return;
      if (preview.newFeeds === 0 && preview.moveCount === 0) {
        await api.cancelOpmlImport();
        const n = preview.inFile;
        this.toast(n ? `Nothing to import: you already follow all ${n === 1 ? 'the feed' : `${n} feeds`} in ${preview.fileName}, in the same folders` : `${preview.fileName} lists no feeds`);
        return;
      }
      this.dialog = { kind: 'import-opml', preview };
    } catch (err) {
      this.error(err);
    }
  }

  /** Imports the file the preview showed. True when it was imported. */
  async finishOpmlImport(layout: ImportLayout): Promise<boolean> {
    const stats = await api.importOpml(layout);
    this.dialog = null;
    await this.reloadTree();
    const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`;
    const parts = [`Added ${plural(stats.feeds, 'feed')}`];
    if (stats.folders) parts[0] += ` and ${plural(stats.folders, 'folder')}`;
    if (stats.moved) parts.push(`moved ${stats.moved}`);
    if (stats.skipped) parts.push(`${stats.skipped} left out`);
    this.toast(parts.join(' · '), 'success');
    return true;
  }

  async exportOpml() {
    try {
      const path = await api.exportOpml();
      if (path) this.toast(`Saved subscriptions to ${path}`, 'success');
    } catch (err) {
      this.error(err);
    }
  }

  // -------------------------------------------------------------------------
  // Backup and restore
  // -------------------------------------------------------------------------

  async backup() {
    try {
      const path = await api.backup();
      if (path) this.toast(`Backed up to ${path}`, 'success');
    } catch (err) {
      this.error(err);
    }
  }

  /** Lets the user pick a backup, then shows what restoring it would do. */
  async chooseBackup() {
    try {
      const summary = await api.chooseBackup();
      if (summary) this.dialog = { kind: 'restore', summary };
    } catch (err) {
      this.error(err);
    }
  }

  /** Restores the backup chosen in `chooseBackup`, then reloads the window. Errors go to the caller. */
  async restoreBackup() {
    const result = await api.restoreBackup();
    // Feed, folder and brief ids now name other rows: forget the tabs and the list
    // that point at them.
    local.set('tabs', []);
    local.set('activeTab', 'main');
    local.set('scope', { kind: 'all' });
    local.set('notice', `Backup restored. The data from before is kept in ${result.previous}`);
    location.reload();
  }

  // -------------------------------------------------------------------------
  // Settings
  // -------------------------------------------------------------------------

  async updateSettings(patch: Partial<Settings>): Promise<boolean> {
    const previous = $state.snapshot(this.settings);
    const next = { ...previous, ...patch };
    this.settings = next;
    this.applyAppearance();
    try {
      this.settings = await api.saveSettings(next);
      this.applyAppearance();
      if ('loadImages' in patch) this.loadArticles();
      if ('videoPlayer' in patch) this.findVideoPlayer();
      return true;
    } catch (err) {
      this.settings = previous;
      this.applyAppearance();
      this.error(err);
      return false;
    }
  }

  // -------------------------------------------------------------------------
  // Native menu (macOS)
  // -------------------------------------------------------------------------

  onMenu(id: string) {
    switch (id) {
      case 'settings':
        this.dialog = { kind: 'settings' };
        break;
      case 'add-feed':
        this.addFeed();
        break;
      case 'new-folder':
        this.dialog = { kind: 'folder', folderId: null, parentId: null };
        break;
      case 'import-opml':
        this.importOpml();
        break;
      case 'export-opml':
        this.exportOpml();
        break;
      case 'backup':
        this.backup();
        break;
      case 'restore-backup':
        this.chooseBackup();
        break;
      case 'export-starred':
        this.dialog = { kind: 'export-starred' };
        break;
      case 'refresh-all':
        this.refresh();
        break;
      case 'close-tab':
        this.closeTab(this.activeTabId);
        break;
      case 'find':
        this.activeTabId = 'main';
        this.searchFocus++;
        break;
      case 'shortcuts':
        this.dialog = { kind: 'shortcuts' };
        break;
      case 'mark-all-read':
        this.markAllRead();
        break;
      case 'view-widescreen':
      case 'view-normal':
      case 'view-combined':
        this.updateSettings({ viewMode: id.slice(5) as Settings['viewMode'] });
        break;
    }
  }
}

export const store = new Store();
