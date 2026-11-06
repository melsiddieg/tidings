import type { Feed, Folder, FolderNode, RootNode, Tree } from './types';

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
  return `${Math.round(diff / DAY)} days ago`;
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

export function buildTree(tree: Tree): RootNode {
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
  const sortFolders = (a: Folder, b: Folder) => a.position - b.position || a.title.localeCompare(b.title);
  const sortFeeds = (a: Feed, b: Feed) => a.position - b.position || a.title.localeCompare(b.title);
  const seen = new Set<number>();
  const build = (folder: Folder): FolderNode => {
    seen.add(folder.id);
    return {
      folder,
      folders: (byParent.get(folder.id) ?? [])
        .filter((f) => !seen.has(f.id))
        .sort(sortFolders)
        .map(build),
      feeds: (feedsByFolder.get(folder.id) ?? []).sort(sortFeeds),
    };
  };
  return {
    folders: (byParent.get(null) ?? []).sort(sortFolders).map(build),
    feeds: (feedsByFolder.get(null) ?? []).sort(sortFeeds),
  };
}

/** Feeds in the same order the sidebar shows them. */
export function flattenFeeds(root: RootNode): Feed[] {
  const out: Feed[] = [];
  const walk = (node: FolderNode) => {
    node.folders.forEach(walk);
    out.push(...node.feeds);
  };
  root.folders.forEach(walk);
  out.push(...root.feeds);
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
    node.folders.forEach((child) => walk(child, depth + 1));
  };
  root.folders.forEach((n) => walk(n, 0));
  return out;
}
