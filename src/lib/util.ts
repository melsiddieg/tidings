import type { Feed, Folder, FolderNode, ItemRef, RootNode, TreeChild, Tree, TreeSort, TreeSortKey } from './types';
import { MANUAL_SORT } from './filters';

const DAY = 86_400_000;

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Short time for list rows: "now", "12m", "3h", "Yesterday", "Mon", "Mar 4", "Mar 4, 2023". */
export function shortTime(seconds: number, now = Date.now()): string {
  const ms = seconds * 1000;
  const diff = now - ms;
  if (diff < 60_000) return 'now';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m`;
  const today = startOfDay(new Date(now));
  if (ms >= today) return `${Math.floor(diff / 3_600_000)}h`;
  if (ms >= today - DAY) return 'Yesterday';
  const d = new Date(ms);
  if (ms >= today - 6 * DAY) return d.toLocaleDateString(undefined, { weekday: 'short' });
  if (d.getFullYear() === new Date(now).getFullYear())
    return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

export function longTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString(undefined, {
    weekday: 'long',
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

export function relativeAgo(ms: number, now = Date.now()): string {
  const diff = Math.max(0, now - ms);
  if (diff < 45_000) return 'just now';
  if (diff < 3_600_000) return `${Math.round(diff / 60_000)} min ago`;
  if (diff < DAY) return `${Math.round(diff / 3_600_000)} h ago`;
  if (diff < 60 * DAY) return `${Math.round(diff / DAY)} days ago`;
  if (diff < 365 * DAY) return `${Math.round(diff / (30 * DAY))} months ago`;
  const years = Math.round(diff / (365 * DAY));
  return years === 1 ? 'a year ago' : `${years} years ago`;
}

/** Group label for date headers in the article list. */
export function dayGroup(seconds: number, now = Date.now()): string {
  const ms = seconds * 1000;
  const today = startOfDay(new Date(now));
  if (ms >= today) return 'Today';
  if (ms >= today - DAY) return 'Yesterday';
  if (ms >= today - 6 * DAY) return 'This week';
  if (ms >= today - 30 * DAY) return 'This month';
  const d = new Date(ms);
  return d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
}

export function countLabel(n: number): string {
  if (n <= 0) return '';
  return n > 999 ? '999+' : String(n);
}

const VIDEO_SITES = ['youtube.com', 'youtube-nocookie.com', 'youtu.be', 'vimeo.com', 'dailymotion.com', 'odysee.com'];

/** A link to one video that mpv or IINA (through yt-dlp) can stream: YouTube,
 *  Vimeo, Dailymotion, Odysee and PeerTube watch pages. Channel pages do not count. */
export function isVideoLink(url: string | null | undefined): boolean {
  if (!url) return false;
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  if (u.protocol !== 'https:' && u.protocol !== 'http:') return false;
  const host = u.hostname.toLowerCase();
  const site = VIDEO_SITES.find((d) => host === d || host.endsWith(`.${d}`));
  const path = u.pathname;
  switch (site) {
    case 'youtube.com':
    case 'youtube-nocookie.com':
      return (path === '/watch' && u.searchParams.has('v')) || /^\/(shorts|live|embed)\/[\w-]{6,}/.test(path);
    case 'youtu.be':
      return /^\/[\w-]{6,}/.test(path);
    case 'vimeo.com':
      return /^\/\d+/.test(path);
    case 'dailymotion.com':
      return path.startsWith('/video/');
    case 'odysee.com':
      return /^\/@[^/]+\/[^/]+/.test(path);
  }
  return isPeerTubeLink(url);
}

/** A PeerTube watch page: /w/<short id> or /videos/watch/<uuid> (same rule as peertube::video_id). */
export function isPeerTubeLink(url: string | null | undefined): boolean {
  if (!url) return false;
  try {
    const u = new URL(url);
    if (u.protocol !== 'https:' && u.protocol !== 'http:') return false;
    const path = u.pathname.replace(/\/+$/, '');
    return /^\/(w|videos\/watch)\/[A-Za-z0-9-]{20,40}$/.test(path);
  } catch {
    return false;
  }
}

/** The video of an article and how it plays:
 *  `peertube` and `file` play inside Tidings; `site` (YouTube...) plays in mpv or IINA. */
export type VideoSource = { kind: 'peertube' | 'file' | 'site'; url: string };

export function videoSource(
  link: string | null | undefined,
  enclosureUrl: string | null | undefined,
  enclosureType: string | null | undefined,
): VideoSource | null {
  if (link && isPeerTubeLink(link)) return { kind: 'peertube', url: link };
  if (enclosureUrl && isVideoFile(enclosureType, enclosureUrl)) return { kind: 'file', url: enclosureUrl };
  if (link && isVideoLink(link)) return { kind: 'site', url: link };
  return null;
}

/** A video file, such as the enclosure of a PeerTube feed item. */
export function isVideoFile(type: string | null | undefined, url: string | null | undefined): boolean {
  if (type?.startsWith('video/')) return true;
  return !!url && /\.(mp4|m4v|webm|mkv|mov)(\?|$)/i.test(url);
}

export function hostOf(url: string | null | undefined): string {
  if (!url) return '';
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return '';
  }
}

/** A stable soft color for letter avatars. */
export function avatarHue(text: string): number {
  let h = 0;
  for (let i = 0; i < text.length; i++) h = (h * 31 + text.charCodeAt(i)) >>> 0;
  return h % 360;
}

export function initial(text: string): string {
  const trimmed = text.replace(/^(the|a|an)\s+/i, '').trim();
  const ch = [...trimmed][0] ?? '?';
  return ch.toUpperCase();
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand('copy');
    ta.remove();
    return ok;
  }
}

/** A row of the tree with what it can be sorted by. A folder counts what is inside it. */
interface Entry {
  child: TreeChild;
  id: number;
  folder: boolean;
  position: number;
  title: string;
  added: number;
  unread: number;
  latest: number;
}

export function buildTree(tree: Tree, sort: TreeSort = MANUAL_SORT): RootNode {
  const byParent = new Map<number | null, Folder[]>();
  for (const f of tree.folders) {
    const list = byParent.get(f.parentId) ?? [];
    list.push(f);
    byParent.set(f.parentId, list);
  }
  const feedsByFolder = new Map<number | null, Feed[]>();
  for (const f of tree.feeds) {
    const list = feedsByFolder.get(f.folderId) ?? [];
    list.push(f);
    feedsByFolder.set(f.folderId, list);
  }
  const compare = treeOrder(sort);
  const seen = new Set<number>();
  const childrenOf = (parent: number | null): Entry[] => {
    const folders = (byParent.get(parent) ?? []).filter((f) => !seen.has(f.id));
    folders.forEach((f) => seen.add(f.id));
    const entries: Entry[] = [
      ...folders.map((f): Entry => {
        const inside = childrenOf(f.id);
        return {
          child: { kind: 'folder', node: { folder: f, children: inside.map((e) => e.child) } },
          id: f.id,
          folder: true,
          position: f.position,
          title: f.title,
          // A new folder always gets a higher id than every folder there is, so ids
          // are in the order the folders were made.
          added: f.id,
          unread: inside.reduce((n, e) => n + e.unread, 0),
          latest: inside.reduce((n, e) => Math.max(n, e.latest), 0),
        };
      }),
      ...(feedsByFolder.get(parent) ?? []).map(
        (f): Entry => ({
          child: { kind: 'feed', feed: f },
          id: f.id,
          folder: false,
          position: f.position,
          title: f.title,
          added: f.createdAt,
          unread: f.unread,
          latest: f.newest ?? 0,
        }),
      ),
    ];
    return entries.sort(compare);
  };
  return { children: childrenOf(null).map((e) => e.child) };
}

/** How two rows with the same parent are ordered. */
function treeOrder(sort: TreeSort): (a: Entry, b: Entry) => number {
  // Manual: feeds and folders share one order; on a tie, folders come first.
  const manual = (a: Entry, b: Entry) =>
    a.position - b.position || Number(b.folder) - Number(a.folder) || a.title.localeCompare(b.title);
  if (sort.key === 'manual') return manual;
  const dir = sort.descending ? -1 : 1;
  const byName = (a: Entry, b: Entry) => a.title.localeCompare(b.title, undefined, { numeric: true, sensitivity: 'base' });
  const by: Record<Exclude<TreeSortKey, 'manual'>, (a: Entry, b: Entry) => number> = {
    name: (a, b) => dir * (byName(a, b) || a.id - b.id),
    added: (a, b) => dir * (a.added - b.added || a.id - b.id),
    unread: (a, b) => dir * (a.unread - b.unread) || byName(a, b),
    latest: (a, b) => dir * (a.latest - b.latest) || byName(a, b),
  };
  const sorted = by[sort.key];
  // Sorted: folders first, then feeds.
  return (a, b) => {
    if (a.folder !== b.folder) return a.folder ? -1 : 1;
    if (!a.folder && !sort.feeds) return manual(a, b);
    return sorted(a, b);
  };
}

export function childKey(child: TreeChild): string {
  return child.kind === 'folder' ? `folder:${child.node.folder.id}` : `feed:${child.feed.id}`;
}

export function childRef(child: TreeChild): ItemRef {
  return child.kind === 'folder' ? { kind: 'folder', id: child.node.folder.id } : { kind: 'feed', id: child.feed.id };
}

export function sameItem(a: ItemRef | null | undefined, b: ItemRef | null | undefined): boolean {
  return !!a && !!b && a.kind === b.kind && a.id === b.id;
}

/** Feeds in the same order the sidebar shows them. */
export function flattenFeeds(root: RootNode): Feed[] {
  const out: Feed[] = [];
  const walk = (children: TreeChild[]) => {
    for (const c of children) {
      if (c.kind === 'feed') out.push(c.feed);
      else walk(c.node.children);
    }
  };
  walk(root.children);
  return out;
}

export function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable;
}

export function enclosureLabel(type: string | null): string {
  if (!type) return 'Attachment';
  if (type.startsWith('audio/')) return 'Audio';
  if (type.startsWith('video/')) return 'Video';
  if (type.includes('pdf')) return 'PDF';
  return 'Attachment';
}

/** Folders in tree order with their depth, for <select> menus. */
export function folderOptions(root: RootNode, exclude?: number): { id: number; title: string; depth: number }[] {
  const out: { id: number; title: string; depth: number }[] = [];
  const walk = (node: FolderNode, depth: number) => {
    if (node.folder.id === exclude) return;
    out.push({ id: node.folder.id, title: node.folder.title, depth });
    for (const c of node.children) if (c.kind === 'folder') walk(c.node, depth + 1);
  };
  for (const c of root.children) if (c.kind === 'folder') walk(c.node, 0);
  return out;
}
