// Fake backend for `npm run dev` in a normal browser (UI work only).
// It is loaded only in development and never ships in release builds.
import type { ArticleDetail, ArticleItem, Feed, Folder, ListQuery, Scope, Settings } from './types';

type Handler = (p: unknown) => void;
const listeners = new Map<string, Set<Handler>>();

export function listen(event: string, handler: Handler): () => void {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(handler);
  return () => listeners.get(event)?.delete(handler);
}

function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((h) => h(payload));
}

const now = Math.floor(Date.now() / 1000);

const folders: Folder[] = [
  { id: 1, parentId: null, title: 'Technology', position: 0, expanded: true },
  { id: 2, parentId: null, title: 'World News', position: 1, expanded: true },
  { id: 3, parentId: null, title: 'Science', position: 2, expanded: false },
  { id: 4, parentId: 1, title: 'Linux & Free Software', position: 0, expanded: true },
];

function feed(id: number, folderId: number | null, title: string, url: string, extra: Partial<Feed> = {}): Feed {
  return {
    id, folderId, title, feedTitle: title, customTitle: null, url, siteUrl: url.replace(/\/feed.*|\/rss.*/, ''),
    description: null, position: id, iconVersion: null, updateInterval: null, retentionMode: null,
    retentionValue: null, fullText: false, paused: false, lastFetched: now - 600, lastSuccess: now - 600,
    lastError: null, unread: 0, total: 0, ...extra,
  };
}

const feeds: Feed[] = [
  feed(1, 1, 'Ars Technica', 'https://feeds.arstechnica.com/arstechnica/index'),
  feed(2, 1, 'The Verge', 'https://www.theverge.com/rss/index.xml'),
  feed(3, 4, 'LWN.net', 'https://lwn.net/headlines/rss'),
  feed(4, 4, 'Planet KDE', 'https://planet.kde.org/global/atom.xml'),
  feed(5, 2, 'BBC News · World', 'https://feeds.bbci.co.uk/news/world/rss.xml'),
  feed(6, 2, 'Al Jazeera', 'https://www.aljazeera.com/xml/rss/all.xml'),
  feed(7, 3, 'Quanta Magazine', 'https://www.quantamagazine.org/feed/'),
  feed(8, 3, 'NASA Image of the Day', 'https://www.nasa.gov/rss/image_of_the_day.rss'),
  feed(9, null, 'Daring Fireball', 'https://daringfireball.net/feeds/main'),
  feed(10, null, 'Julia Evans', 'https://jvns.ca/atom.xml', { lastError: 'The server took too long to respond' }),
];

const TITLES = [
  'The quiet revolution in open-source hardware design',
  'Why your next laptop might run on a RISC-V chip',
  'Inside the team rebuilding the Linux scheduler from scratch',
  'KDE Plasma 6.4 brings smarter window tiling and HDR improvements',
  'Leaders meet in Geneva as ceasefire talks enter a third week',
  'A record harvest season, and the farmers who made it happen',
  'Mathematicians finally prove a 60-year-old conjecture about knots',
  'A spiral galaxy, seen edge-on, glows with newborn stars',
  'On the virtue of boring technology',
  'How DNS actually works, explained with comics',
  'The browser engines that power almost everything you read',
  'What the new EU rules mean for app stores',
  'Kernel 6.12 lands with real-time support merged at last',
  'Akregator turns 20: a look back at the KDE feed reader',
  'Satellite images show the scale of spring flooding',
  'Why the world’s oldest trees keep surprising scientists',
  'Quantum error correction passes an important milestone',
  'A small team, a big telescope, and a very long night',
  'Notes on writing software that lasts',
  'Debugging a mysterious network slowdown, step by step',
  'The economics of keeping old phones working',
  'Electric ferries are quietly changing how cities move',
  'GNOME and KDE agree on a shared notifications spec',
  'تقرير: الطاقة الشمسية تتصدر الاستثمارات الجديدة في المنطقة',
];

const AUTHORS = ['Jane Doe', 'Ahmed Yousif', 'Maria Garcia', 'Lee Chen', null, 'Sam Patel'];

function svgImage(hue: number, w = 1200, h = 675): string {
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' width='${w}' height='${h}' viewBox='0 0 ${w} ${h}'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='hsl(${hue},70%,62%)'/><stop offset='1' stop-color='hsl(${(hue + 50) % 360},65%,40%)'/></linearGradient></defs><rect width='100%' height='100%' fill='url(#g)'/><circle cx='${w * 0.72}' cy='${h * 0.3}' r='${h * 0.18}' fill='rgba(255,255,255,.25)'/><path d='M0 ${h * 0.75} Q ${w * 0.3} ${h * 0.55} ${w * 0.6} ${h * 0.72} T ${w} ${h * 0.65} V ${h} H 0Z' fill='rgba(0,0,0,.18)'/></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function body(title: string, i: number): string {
  if (/[؀-ۿ]/.test(title)) {
    return `<p>أظهرت بيانات جديدة أن مشاريع الطاقة الشمسية استحوذت على الحصة الأكبر من الاستثمارات في قطاع الطاقة خلال العام الماضي، مدفوعة بانخفاض التكاليف وتحسن كفاءة الألواح.</p><p>ويقول خبراء إن هذا التحول سيستمر خلال العقد المقبل، مع توسع شبكات النقل وتخزين الطاقة.</p><blockquote>«التحول لم يعد خيارًا، بل أصبح ضرورة اقتصادية»، قال أحد المحللين.</blockquote>`;
  }
  return `
<p>${title}. It is a story that started small and grew into something much larger than anyone expected. Over the past year, a handful of people have been working on it in the open, publishing their notes, their mistakes and their progress.</p>
<figure><img src="${svgImage((i * 47) % 360)}" alt=""><figcaption>An illustration of the idea. Images in Tidings are loaded through a private proxy.</figcaption></figure>
<p>The core idea is simple: <strong>do less, but do it well</strong>. Instead of chasing every new feature, the team focused on reliability, clear interfaces and documentation that people actually read. You can <a href="https://example.com/article-${i}">read the original announcement</a> for the full background.</p>
<h2>What changed</h2>
<ul><li>Startup time dropped by more than half.</li><li>Memory use is now predictable under load.</li><li>Configuration moved to a single, documented file.</li></ul>
<blockquote>“We stopped adding knobs and started removing them. Every option we removed made the software easier to trust.”</blockquote>
<p>For developers, the most visible change is the new API. A minimal example looks like this:</p>
<pre><code>let feed = reader.subscribe("https://example.com/feed.xml")?;
for article in feed.unread() {
    println!("{}", article.title);
}</code></pre>
<p>There is still work to do, and the roadmap is public. If you want to follow along, the project posts weekly updates, and of course it has an RSS feed.</p>`;
}

let articles: (ArticleItem & { html: string; full?: string })[] = [];
for (let i = 0; i < 64; i++) {
  const f = feeds[i % feeds.length];
  const title = TITLES[i % TITLES.length];
  articles.push({
    id: i + 1,
    feedId: f.id,
    title,
    url: `https://example.com/${f.id}/article-${i}`,
    author: AUTHORS[i % AUTHORS.length],
    excerpt: `${title}. It is a story that started small and grew into something much larger than anyone expected. Over the past year, a handful of people have been working on it in the open.`,
    published: now - i * 5400 - (i % 3) * 1200,
    isRead: i > 9 && i % 3 !== 0,
    isStarred: i % 11 === 2,
    isNew: i < 4,
    hasEnclosure: i % 13 === 5,
    thumb: i % 4 === 1 ? null : svgImage((i * 47) % 360, 160, 160),
    html: body(title, i),
  });
}

let settings: Settings = {
  refreshInterval: 60, refreshOnStartup: true, retentionMode: 'count', retentionCount: 1000, retentionDays: 90,
  loadImages: true, stripTracking: true, httpsOnly: false, proxy: '',
  userAgent: 'Mozilla/5.0 (compatible; Tidings/0.1; feed reader)', markReadDelay: 0, linkTarget: 'tab',
  theme: 'system', accent: 'ember', readerFont: 'serif', readerSize: 18, viewMode: 'widescreen', dockBadge: true,
  concurrency: 6,
};

function counts() {
  for (const f of feeds) {
    f.unread = articles.filter((a) => a.feedId === f.id && !a.isRead).length;
    f.total = articles.filter((a) => a.feedId === f.id).length;
  }
}

function inScope(a: ArticleItem, scope: Scope): boolean {
  if (scope.kind === 'all') return true;
  if (scope.kind === 'starred') return a.isStarred;
  if (scope.kind === 'feed') return a.feedId === scope.id;
  const ids = new Set<number>([scope.id]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const f of folders) if (f.parentId != null && ids.has(f.parentId) && !ids.has(f.id)) { ids.add(f.id); grew = true; }
  }
  const folderId = feeds.find((f) => f.id === a.feedId)?.folderId;
  return folderId != null && ids.has(folderId);
}

function query(q: ListQuery) {
  const words = q.query.toLowerCase().split(/\s+/).filter(Boolean);
  return articles
    .filter((a) => inScope(a, q.scope))
    .filter((a) => (q.filter === 'unread' ? !a.isRead : q.filter === 'new' ? a.isNew : q.filter === 'starred' ? a.isStarred : true))
    .filter((a) => words.every((w) => `${a.title} ${a.excerpt}`.toLowerCase().includes(w)))
    .sort((a, b) => (q.oldestFirst ? a.published - b.published : b.published - a.published))
    .slice(q.offset, q.offset + q.limit);
}

function detail(a: (typeof articles)[number], full = false, images = settings.loadImages): ArticleDetail {
  let html = full && a.full ? a.full : a.html;
  let blocked = 0;
  if (!images) {
    blocked = (html.match(/<img/g) ?? []).length;
    html = html.replace(/<img[^>]*>/g, '');
  }
  return {
    id: a.id, feedId: a.feedId, title: a.title, url: a.url, author: a.author, published: a.published,
    isRead: a.isRead, isStarred: a.isStarred, isNew: a.isNew, html, blockedImages: blocked,
    showingFullText: full && !!a.full, hasFullText: !!a.full,
    enclosureUrl: a.hasEnclosure ? 'https://cdn.example.com/episode.mp3' : null,
    enclosureType: a.hasEnclosure ? 'audio/mpeg' : null,
  };
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
let nextId = 1000;

export async function invoke(cmd: string, args: Record<string, any> = {}): Promise<unknown> {
  await wait(30);
  switch (cmd) {
    case 'get_tree':
      counts();
      return structuredClone({ folders, feeds, starred: articles.filter((a) => a.isStarred).length });
    case 'get_counts':
      counts();
      return { feeds: feeds.map((f) => [f.id, f.unread]), starred: articles.filter((a) => a.isStarred).length };
    case 'list_articles':
      return query(args.query).map(({ html: _h, full: _f, ...item }) => ({ ...item, thumb: settings.loadImages ? item.thumb : null }));
    case 'list_articles_full':
      return query(args.query).map((a) => detail(a, false, args.loadImages));
    case 'get_article': {
      const a = articles.find((x) => x.id === args.id);
      if (!a) throw 'Article not found';
      return detail(a, !!args.fullText, args.loadImages ?? settings.loadImages);
    }
    case 'fetch_full_text': {
      await wait(700);
      const a = articles.find((x) => x.id === args.id)!;
      a.full = a.html + '<h2>More from the full article</h2><p>This paragraph was extracted from the web page in reader mode. In the real app, Tidings downloads the page in Rust, keeps only the article text and removes everything else.</p>';
      return detail(a, true, args.loadImages ?? settings.loadImages);
    }
    case 'read_url':
      await wait(600);
      return {
        url: args.url, title: 'A page opened in reader mode', byline: 'By A. Writer', siteName: new URL(args.url).hostname,
        excerpt: null, published: new Date().toISOString(), dir: null, blockedImages: 0,
        html: '<p>This is how a linked web page looks in a reader tab. No scripts run, no cookies are stored and trackers never see you.</p>' + body('Reader mode', 3),
      };
    case 'set_read':
      for (const a of articles) if (args.ids.includes(a.id)) { a.isRead = args.read; if (args.read) a.isNew = false; }
      return null;
    case 'set_starred': {
      const a = articles.find((x) => x.id === args.id);
      if (a) a.isStarred = args.starred;
      return null;
    }
    case 'mark_all_read': {
      let n = 0;
      for (const a of articles) if (inScope(a, args.scope) && !a.isRead) { a.isRead = true; n++; }
      return n;
    }
    case 'delete_articles':
      articles = articles.filter((a) => !args.ids.includes(a.id));
      return null;
    case 'discover_feeds':
      await wait(500);
      if (!String(args.url).includes('.')) throw 'This is not a valid web address';
      return [{ url: `https://${String(args.url).replace(/^https?:\/\//, '')}/feed.xml`, title: 'Example Blog', kind: 'RSS' }];
    case 'add_feed': {
      const id = ++nextId;
      feeds.push(feed(id, args.folderId ?? null, args.title ?? 'New feed', args.url));
      setTimeout(() => emit('feed-updated', { feedId: id, newArticles: 0, error: null }), 800);
      return id;
    }
    case 'update_feed': {
      const f = feeds.find((x) => x.id === args.id)!;
      Object.assign(f, args.patch, { title: args.patch.customTitle || f.feedTitle });
      return null;
    }
    case 'delete_feed':
      feeds.splice(feeds.findIndex((f) => f.id === args.id), 1);
      articles = articles.filter((a) => a.feedId !== args.id);
      return null;
    case 'move_feed': {
      const f = feeds.find((x) => x.id === args.id)!;
      f.folderId = args.folderId;
      return null;
    }
    case 'create_folder':
      folders.push({ id: ++nextId, parentId: args.parentId ?? null, title: args.title, position: 99, expanded: true });
      return nextId;
    case 'rename_folder':
      folders.find((f) => f.id === args.id)!.title = args.title;
      return null;
    case 'delete_folder':
      folders.splice(folders.findIndex((f) => f.id === args.id), 1);
      return null;
    case 'move_folder': {
      const moving = folders.find((f) => f.id === args.id)!;
      moving.parentId = args.parentId ?? null;
      const siblings = folders
        .filter((f) => f.parentId === moving.parentId && f.id !== moving.id)
        .sort((a, b) => a.position - b.position);
      const at = args.beforeId == null ? -1 : siblings.findIndex((f) => f.id === args.beforeId);
      siblings.splice(at < 0 ? siblings.length : at, 0, moving);
      siblings.forEach((f, i) => (f.position = i));
      return null;
    }
    case 'set_folder_expanded':
      folders.find((f) => f.id === args.id)!.expanded = args.expanded;
      return null;
    case 'refresh_feeds': {
      const total = args.feedIds?.length ?? feeds.length;
      let done = 0;
      emit('refresh-progress', { done, total });
      const tick = setInterval(() => {
        done++;
        emit('refresh-progress', { done, total });
        if (done >= total) clearInterval(tick);
      }, 180);
      return total;
    }
    case 'import_opml':
      return { feeds: 0, folders: 0, skipped: 0 };
    case 'export_opml':
      return null;
    case 'get_settings':
      return { ...settings };
    case 'save_settings':
      settings = { ...args.settings };
      return { ...settings };
    case 'open_external':
      window.open(args.url, '_blank', 'noopener');
      return null;
    case 'set_badge':
      return null;
    case 'app_info':
      return { version: '0.1.0', dataDir: '~/Library/Application Support/org.tidings.reader', platform: 'web' };
  }
  throw `Unknown command ${cmd}`;
}
