import type { Feed, Folder, FolderNode, ItemRef, RootNode, TreeChild, Tree } from './types';

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
  const seen = new Set<number>();
  // Feeds and folders share one order; on a tie, folders come first.
  const childrenOf = (parent: number | null): TreeChild[] => {
    const folders = (byParent.get(parent) ?? []).filter((f) => !seen.has(f.id));
    folders.forEach((f) => seen.add(f.id));
    const items: [number, number, string, () => TreeChild][] = [
      ...folders.map((f): [number, number, string, () => TreeChild] => [f.position, 0, f.title, () => ({ kind: 'folder', node: { folder: f, children: childrenOf(f.id) } })]),
      ...(feedsByFolder.get(parent) ?? []).map((f): [number, number, string, () => TreeChild] => [f.position, 1, f.title, () => ({ kind: 'feed', feed: f })]),
    ];
    items.sort((a, b) => a[0] - b[0] || a[1] - b[1] || a[2].localeCompare(b[2]));
    return items.map((i) => i[3]());
  };
  return { children: childrenOf(null) };
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
