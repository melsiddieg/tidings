// Application state and actions. Components read from `store` and call its methods.
import { api, errorText, on } from './api';
import type {
  AppInfo,
  ArticleItem,
  Dialog,
  Feed,
  FeedUpdated,
  Filter,
  FolderNode,
  ItemRef,
  TreeChild,
  ListQuery,
  MenuItem,
  RefreshProgress,
  Scope,
  Settings,
  Tab,
  Tree,
} from './types';
import { buildTree, childRef, flattenFeeds, sameItem } from './util';

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

  scope = $state<Scope>(local.get<Scope>('scope', { kind: 'all' }));
  filter = $state<Filter>(local.get<Filter>('filter', 'all'));
  query = $state('');
  oldestFirst = $state<boolean>(local.get('oldestFirst', false));

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

  root = $derived(buildTree(this.tree));
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
    return this.folderById.get(s.id)?.title ?? 'Folder';
  });
  scopeUnread = $derived.by(() => {
    const s = this.scope;
    if (s.kind === 'all') return this.unreadTotal;
    if (s.kind === 'starred') return 0;
    if (s.kind === 'feed') return this.feedById.get(s.id)?.unread ?? 0;
    return this.folderUnread.get(s.id) ?? 0;
  });

  private listToken = 0;
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
    } catch (err) {
      this.error(err);
    }
    document.documentElement.dataset.platform = this.info?.platform ?? 'web';
    this.applyAppearance();
    matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => this.applyAppearance());
    try {
      if (!this.scopeExists(this.scope)) this.scope = { kind: 'all' };
      this.restoreTabs();
      await this.loadArticles(false);
      await on<FeedUpdated>('feed-updated', (e) => this.onFeedUpdated(e));
      await on<RefreshProgress>('refresh-progress', (p) => this.onProgress(p));
      await on<string>('menu', (id) => this.onMenu(id));
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
      } catch {
        /* counts are cosmetic */
      }
    }, 120);
  }

  feedIcon(feedId: number): Feed | undefined {
    return this.feedById.get(feedId);
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
    };
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
    local.set('scope', scope);
    this.selectedId = null;
    this.activeTabId = 'main';
    this.loadArticles(false);
  }

  setFilter(filter: Filter) {
    this.filter = filter;
    local.set('filter', filter);
    this.loadArticles();
  }

  setQuery(query: string) {
    this.query = query;
    this.loadArticles();
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

  async markAllRead(scope: Scope = this.scope) {
    try {
      const n = await api.markAllRead($state.snapshot(scope));
      if (JSON.stringify(scope) === JSON.stringify(this.scope)) {
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
    const saved = local.get<Tab[]>('tabs', []).filter((t) => t.kind !== 'main');
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

  openArticleTab(id: number, background = false) {
    const existing = this.tabs.find((t) => t.kind === 'article' && t.articleId === id);
    if (existing) {
      if (!background) this.activateTab(existing.id);
      return;
    }
    const item = this.articles.find((a) => a.id === id);
    this.insertTab(
      { id: `a${id}-${Date.now().toString(36)}`, kind: 'article', articleId: id, feedId: item?.feedId ?? null, title: item?.title ?? 'Article' },
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
    if (url.startsWith('mailto:') || mode === 'browser' || (mode === 'default' && this.settings.linkTarget === 'browser')) {
      this.openExternal(url);
      return;
    }
    this.openWebTab(url, mode === 'background');
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
    return this.refresh();
  }

  watchFeed(id: number) {
    this.watchFeeds.add(id);
  }

  private scopeIncludes(feedId: number): boolean {
    const s = this.scope;
    if (s.kind === 'all') return true;
    if (s.kind === 'starred') return false;
    if (s.kind === 'feed') return s.id === feedId;
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
  async moveItem(item: ItemRef, parentId: number | null, before: ItemRef | null) {
    if (sameItem(item, before)) return;
    try {
      if (item.kind === 'feed') await api.moveFeed(item.id, parentId, before);
      else await api.moveFolder(item.id, parentId, before);
      await this.reloadTree();
    } catch (err) {
      this.error(err);
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

  async importOpml() {
    try {
      const stats = await api.importOpml();
      if (!stats) return;
      await this.reloadTree();
      const parts = [`${stats.feeds} feed${stats.feeds === 1 ? '' : 's'}`];
      if (stats.folders) parts.push(`${stats.folders} folder${stats.folders === 1 ? '' : 's'}`);
      let text = `Imported ${parts.join(' in ')}`;
      if (stats.skipped) text += ` · ${stats.skipped} already subscribed or invalid`;
      this.toast(text, 'success');
    } catch (err) {
      this.error(err);
    }
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
        this.dialog = { kind: 'add-feed', folderId: null };
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
