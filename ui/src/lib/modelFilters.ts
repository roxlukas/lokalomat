import { SPEED_RANK, type Fit, type Modality, type ModelView, type Speed, type VariantView } from './ipc';

export interface ModelFilters {
  query: string;
  modality: Modality | 'all';
  /** Only models with a variant that fits in memory and is not too slow. */
  fitsHardware: boolean;
  commercial: boolean;
  /** PRD §11: on by default. */
  euAllowed: boolean;
  polish: boolean;
  installed: 'all' | 'installed' | 'not_installed';
}

export const DEFAULT_FILTERS: ModelFilters = {
  query: '',
  modality: 'all',
  fitsHardware: false,
  commercial: false,
  euAllowed: true,
  polish: false,
  installed: 'all',
};

const FIT_RANK: Record<Fit, number> = { fits: 3, tight: 2, offload: 1, wont_run: 0 };

export function isInstalled(model: ModelView): boolean {
  return model.variants.some((v) => v.state === 'installed');
}

/** The variant shown in the collapsed row: installed → recommended & fitting → best fitting → first. */
export function primaryVariant(model: ModelView): VariantView | undefined {
  const usable = model.variants.filter((v) => v.supported_here);
  const pool = usable.length ? usable : model.variants;
  return (
    pool.find((v) => v.state === 'installed') ??
    pool.find((v) => v.state !== 'not_installed') ??
    pool.find((v) => v.recommended && (v.fit === null || FIT_RANK[v.fit.fit] >= 2) && speedRank(v) !== 0) ??
    [...pool].sort(
      (a, b) => fitRank(b) - fitRank(a) || speedRank(b) - speedRank(a) || b.memory_estimate_mb - a.memory_estimate_mb,
    )[0]
  );
}

export function fitRank(v: VariantView): number {
  return v.fit ? FIT_RANK[v.fit.fit] : -1;
}

/** 0–3 from too slow to fast; -1 when unknown. */
export function speedRank(v: VariantView): number {
  return v.speed ? SPEED_RANK[v.speed.speed] : -1;
}

/** Fits in memory and is not too slow (unknown speed counts as fine). */
export function runsWell(v: VariantView): boolean {
  const fit = v.fit?.fit;
  return (fit === 'fits' || fit === 'tight') && v.speed?.speed !== 'too_slow';
}

/** Best fit among the variants available on this platform. */
export function bestFit(model: ModelView): Fit | null {
  let best: VariantView | undefined;
  for (const v of model.variants) {
    if (!v.supported_here || !v.fit) continue;
    if (!best || fitRank(v) > fitRank(best)) best = v;
  }
  return best?.fit?.fit ?? null;
}

function matchesQuery(model: ModelView, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [model.name, model.publisher, model.id, ...model.tags, model.description_pl, model.description_en]
    .join(' ')
    .toLowerCase()
    .includes(q);
}

export function filterModels(list: ModelView[], f: ModelFilters): ModelView[] {
  return list.filter((m) => {
    if (f.modality !== 'all' && m.modality !== f.modality) return false;
    if (f.euAllowed && !m.license.eu_allowed) return false;
    if (f.commercial && m.license.commercial !== 'yes') return false;
    if (f.polish && !m.languages.includes('pl')) return false;
    if (f.fitsHardware && !m.variants.some((v) => v.supported_here && runsWell(v))) return false;
    if (f.installed === 'installed' && !isInstalled(m)) return false;
    if (f.installed === 'not_installed' && isInstalled(m)) return false;
    return matchesQuery(m, f.query);
  });
}

/** Number of filters that differ from the defaults (for a "reset" affordance). */
export function activeFilterCount(f: ModelFilters): number {
  return (Object.keys(DEFAULT_FILTERS) as (keyof ModelFilters)[]).filter((k) => f[k] !== DEFAULT_FILTERS[k]).length;
}

/**
 * Accepts `org/repo` + path, or a pasted Hugging Face URL such as
 * `https://huggingface.co/org/repo/blob/main/dir/model.gguf`.
 */
export function parseHfReference(input: string): { repo: string; path: string } | null {
  const text = input.trim();
  const url = text.match(/^https?:\/\/(?:www\.)?huggingface\.co\/([^/]+\/[^/]+)\/(?:blob|resolve)\/[^/]+\/(.+?)(?:\?.*)?$/i);
  if (url) return { repo: url[1]!, path: decodeURIComponent(url[2]!) };
  const short = text.match(/^([\w.-]+\/[\w.-]+)\/(.+\.gguf)$/i);
  if (short) return { repo: short[1]!, path: short[2]! };
  return null;
}

export type Capability = 'chat' | 'image' | 'stt' | 'tts' | 'cloning' | 'music';
export const CAPABILITIES: Capability[] = ['chat', 'image', 'stt', 'tts', 'cloning', 'music'];

/**
 * Wizard summary level: the speed of the best model (ADR 0016), or why
 * nothing runs – no graphics card for images, or not enough memory.
 */
export type CapLevel = Speed | 'needs_gpu' | 'wont_run';

/** Speed of a variant; before the speed estimate existed, derived from the memory fit. */
function variantLevel(v: VariantView): CapLevel | null {
  if (v.speed) {
    if (v.speed.reason === 'needs_gpu') return 'needs_gpu';
    if (v.speed.reason === 'not_enough_memory') return 'wont_run';
    return v.speed.speed;
  }
  if (!v.fit) return null;
  return ({ fits: 'fast', tight: 'usable', offload: 'slow', wont_run: 'wont_run' } as const)[v.fit.fit];
}

const CAP_RANK: Record<CapLevel, number> = { ...SPEED_RANK, needs_gpu: -1, wont_run: -2 };

/** For the wizard summary: the best level any catalog model reaches per capability. */
export function capabilityFits(list: ModelView[]): Record<Capability, CapLevel | null> {
  const of = (pred: (m: ModelView) => boolean): CapLevel | null => {
    let best: CapLevel | null = null;
    for (const m of list.filter((x) => !x.custom && pred(x))) {
      for (const v of m.variants.filter((x) => x.supported_here)) {
        const level = variantLevel(v);
        if (level && (best === null || CAP_RANK[level] > CAP_RANK[best])) best = level;
      }
    }
    return best;
  };
  const cloning = (m: ModelView) => m.features.includes('voice_cloning');
  return {
    chat: of((m) => m.modality === 'llm'),
    image: of((m) => m.modality === 'image'),
    stt: of((m) => m.modality === 'stt'),
    tts: of((m) => m.modality === 'tts' && !cloning(m)),
    cloning: of((m) => m.modality === 'tts' && cloning(m)),
    music: of((m) => m.modality === 'music'),
  };
}

/** The capabilities the catalog has at least one model for (the free edition has no image or speech models). */
export function offeredCapabilities(list: ModelView[]): Capability[] {
  const has = (pred: (m: ModelView) => boolean) => list.some((m) => !m.custom && pred(m));
  const cloning = (m: ModelView) => m.features.includes('voice_cloning');
  const offered: Record<Capability, boolean> = {
    chat: has((m) => m.modality === 'llm'),
    image: has((m) => m.modality === 'image'),
    stt: has((m) => m.modality === 'stt'),
    tts: has((m) => m.modality === 'tts' && !cloning(m)),
    cloning: has((m) => m.modality === 'tts' && cloning(m)),
    music: has((m) => m.modality === 'music'),
  };
  return CAPABILITIES.filter((c) => offered[c]);
}

/** `default` keeps catalogue order; `cat:<name>` sorts by a benchmark category. */
export type SortKey = 'default' | 'overall' | 'speed' | `cat:${string}`;

export function isSortKey(value: string): value is SortKey {
  return value === 'default' || value === 'overall' || value === 'speed' || value.startsWith('cat:');
}

/** Benchmark value used for sorting, `null` when the model was not tested. */
export function scoreFor(model: ModelView, key: SortKey): number | null {
  const s = model.scores;
  if (!s || key === 'default') return null;
  if (key === 'overall') return s.overall;
  if (key === 'speed') return s.tokens_per_second;
  return s.categories[key.slice(4)]?.avg ?? null;
}

/** Highest score first; models without a score keep their order at the end. */
export function sortModels(list: ModelView[], key: SortKey): ModelView[] {
  if (key === 'default') return list;
  return list
    .map((m, i) => ({ m, i, s: scoreFor(m, key) }))
    .sort((a, b) => {
      if (a.s === null && b.s === null) return a.i - b.i;
      if (a.s === null) return 1;
      if (b.s === null) return -1;
      return b.s - a.s || a.i - b.i;
    })
    .map((x) => x.m);
}
