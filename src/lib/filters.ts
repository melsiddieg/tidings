// Feed types and date ranges: the two ways to narrow the article list besides
// folders and read state.
import type { DateRange, FeedType, TranslateLang, TreeSort, TreeSortKey } from './types';

export interface TypeInfo {
  value: FeedType;
  /** Plural, for the sidebar and list title. */
  label: string;
  /** Singular, for a single feed. */
  one: string;
  icon: string;
}

export const FEED_TYPES: TypeInfo[] = [
  { value: 'podcast', label: 'Podcasts', one: 'Podcast', icon: 'headphones' },
  { value: 'video', label: 'Videos', one: 'Video', icon: 'video' },
  { value: 'code', label: 'Code & releases', one: 'Code & releases', icon: 'code' },
  { value: 'discussion', label: 'Discussions', one: 'Discussion', icon: 'message' },
  { value: 'newsletter', label: 'Newsletters', one: 'Newsletter', icon: 'mail' },
  { value: 'article', label: 'Articles', one: 'Article', icon: 'file-text' },
];

export function typeInfo(type: FeedType): TypeInfo {
  return FEED_TYPES.find((t) => t.value === type) ?? FEED_TYPES[FEED_TYPES.length - 1];
}

/** The languages language.rs tells apart: the name in English, and the language's
 *  own name for the menu. "und" holds the rest (and text that shows no language). */
export const LANGUAGES: Record<string, { name: string; own: string }> = {
  en: { name: 'English', own: 'English' },
  fr: { name: 'French', own: 'Français' },
  de: { name: 'German', own: 'Deutsch' },
  es: { name: 'Spanish', own: 'Español' },
  pt: { name: 'Portuguese', own: 'Português' },
  it: { name: 'Italian', own: 'Italiano' },
  nl: { name: 'Dutch', own: 'Nederlands' },
  ar: { name: 'Arabic', own: 'العربية' },
  fa: { name: 'Persian', own: 'فارسی' },
  ur: { name: 'Urdu', own: 'اردو' },
  zh: { name: 'Chinese', own: '中文' },
  ja: { name: 'Japanese', own: '日本語' },
  ko: { name: 'Korean', own: '한국어' },
  ru: { name: 'Russian', own: 'Русский' },
  uk: { name: 'Ukrainian', own: 'Українська' },
  el: { name: 'Greek', own: 'Ελληνικά' },
  he: { name: 'Hebrew', own: 'עברית' },
  th: { name: 'Thai', own: 'ไทย' },
  hi: { name: 'Hindi', own: 'हिन्दी' },
  und: { name: 'Other', own: 'Other' },
};

export const LANGUAGE_NAMES: Record<string, string> = Object.fromEntries(
  Object.entries(LANGUAGES).map(([code, l]) => [code, l.name]),
);

/** "Arabic", for the list header; the code itself for one Tidings does not know. */
export function languageName(code: string): string {
  return LANGUAGES[code]?.name ?? code;
}

/** "Arabic · العربية", for the language menu. */
export function languageLabel(code: string): string {
  const l = LANGUAGES[code];
  if (!l) return code;
  return l.own === l.name ? l.name : `${l.name} · ${l.own}`;
}

/** The languages an article can be translated into, the usual one first. Into
 *  English: Chinese and Arabic articles. Into Arabic: every article not in Arabic,
 *  but only when Arabic is the usual language or the article is Chinese, so English
 *  readers do not get a Translate button on every English article. */
export function translationTargets(language: '' | 'zh' | 'ar', usual: TranslateLang): TranslateLang[] {
  const english = language === 'zh' || language === 'ar';
  const arabic = language !== 'ar' && (usual === 'ar' || language === 'zh');
  const all: TranslateLang[] = [...(english ? (['en'] as const) : []), ...(arabic ? (['ar'] as const) : [])];
  return all.sort((a, b) => Number(b === usual) - Number(a === usual));
}

/** Every language an article can be translated into, whatever the usual one: the
 *  Translate button's menu. English only for Chinese and Arabic articles (an English
 *  article would come back as it is); Arabic for everything not in Arabic. */
export function possibleTargets(language: '' | 'zh' | 'ar'): TranslateLang[] {
  return language === 'zh' ? ['en', 'ar'] : language === 'ar' ? ['en'] : ['ar'];
}

/** Ways to sort the sidebar, with what ascending and descending mean for each. */
export const SORT_KEYS: { key: TreeSortKey; label: string; up: string; down: string }[] = [
  { key: 'manual', label: 'Manual order', up: '', down: '' },
  { key: 'name', label: 'Name', up: 'A to Z', down: 'Z to A' },
  { key: 'added', label: 'Date added', up: 'oldest first', down: 'newest first' },
  { key: 'unread', label: 'Unread articles', up: 'fewest first', down: 'most first' },
  { key: 'latest', label: 'Latest article', up: 'oldest first', down: 'newest first' },
];

export const MANUAL_SORT: TreeSort = { key: 'manual', descending: false, feeds: true };

/** A saved sort, or manual order when it is not one Tidings knows. */
export function validSort(value: unknown): TreeSort {
  const v = value as Partial<TreeSort> | null;
  if (!v || typeof v !== 'object' || !SORT_KEYS.some((k) => k.key === v.key)) return MANUAL_SORT;
  return { key: v.key as TreeSortKey, descending: v.descending === true, feeds: v.feeds !== false };
}

/** "Name, A to Z", for the sort button's tooltip. */
export function sortLabel(sort: TreeSort): string {
  const k = SORT_KEYS.find((x) => x.key === sort.key) ?? SORT_KEYS[0];
  return sort.key === 'manual' ? k.label : `${k.label}, ${sort.descending ? k.down : k.up}`;
}

type Preset = DateRange['preset'];

export const DATE_PRESETS: { value: Exclude<Preset, 'custom'>; label: string }[] = [
  { value: 'any', label: 'Any time' },
  { value: 'today', label: 'Today' },
  { value: 'yesterday', label: 'Yesterday' },
  { value: 'week', label: 'Last 7 days' },
  { value: 'month', label: 'Last 30 days' },
  { value: 'year', label: 'This year' },
  { value: 'older', label: 'Older than 30 days' },
];

/** Local midnight, `days` days after `d` (calendar days, so DST changes do not shift it). */
function dayStart(d: Date, days = 0): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days);
}

/** `YYYY-MM-DD` as local midnight. */
export function parseDay(value: string | null | undefined): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value ?? '');
  if (!m) return null;
  const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return Number.isNaN(d.getTime()) ? null : d;
}

/** Local date as `YYYY-MM-DD`, for date inputs. */
export function formatDay(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/**
 * The range as Unix seconds: published at or after `since`, before `until`.
 * Relative presets are worked out from `now`, so they stay right across midnight.
 */
export function rangeBounds(range: DateRange, now = new Date()): { since: number | null; until: number | null } {
  const sec = (d: Date | null) => (d ? Math.floor(d.getTime() / 1000) : null);
  const today = dayStart(now);
  switch (range.preset) {
    case 'today':
      return { since: sec(today), until: null };
    case 'yesterday':
      return { since: sec(dayStart(today, -1)), until: sec(today) };
    case 'week':
      return { since: sec(dayStart(today, -6)), until: null };
    case 'month':
      return { since: sec(dayStart(today, -29)), until: null };
    case 'year':
      return { since: sec(new Date(now.getFullYear(), 0, 1)), until: null };
    case 'older':
      // Exactly what "Last 30 days" leaves out.
      return { since: null, until: sec(dayStart(today, -29)) };
    case 'custom': {
      const from = parseDay(range.from);
      const to = parseDay(range.to);
      return { since: sec(from), until: sec(to ? dayStart(to, 1) : null) };
    }
    default:
      return { since: null, until: null };
  }
}

export function isAnyTime(range: DateRange): boolean {
  if (range.preset === 'custom') return !parseDay(range.from) && !parseDay(range.to);
  return range.preset === 'any';
}

export function rangeLabel(range: DateRange): string {
  if (range.preset !== 'custom') return DATE_PRESETS.find((p) => p.value === range.preset)?.label ?? 'Any time';
  const from = parseDay(range.from);
  const to = parseDay(range.to);
  const thisYear = new Date().getFullYear();
  const short = (d: Date) => d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  const long = (d: Date) => d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  // The year only when it is not this year, and only once for a range within one year.
  const fmt = (d: Date) => (d.getFullYear() === thisYear ? short(d) : long(d));
  if (from && to) {
    if (from.getTime() === to.getTime()) return fmt(from);
    if (from.getFullYear() !== to.getFullYear()) return `${long(from)} – ${long(to)}`;
    return to.getFullYear() === thisYear ? `${short(from)} – ${short(to)}` : `${short(from)} – ${long(to)}`;
  }
  if (from) return `Since ${fmt(from)}`;
  if (to) return `Until ${fmt(to)}`;
  return 'Any time';
}

/** Restores a saved range, falling back to "Any time" for anything unexpected. */
export function validRange(value: unknown): DateRange {
  const v = value as Partial<{ preset: string; from: unknown; to: unknown }> | null;
  if (v && DATE_PRESETS.some((p) => p.value === v.preset)) return { preset: v.preset as Exclude<Preset, 'custom'> };
  if (v?.preset === 'custom') {
    const day = (x: unknown) => (typeof x === 'string' && parseDay(x) ? x : null);
    return { preset: 'custom', from: day(v.from), to: day(v.to) };
  }
  return { preset: 'any' };
}
