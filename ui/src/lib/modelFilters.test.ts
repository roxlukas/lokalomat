import { describe, expect, it } from 'vitest';
import type { ModelView, VariantView } from './ipc';
import {
  activeFilterCount,
  bestFit,
  capabilityFits,
  DEFAULT_FILTERS,
  filterModels,
  isSortKey,
  offeredCapabilities,
  sortModels,
  parseHfReference,
  primaryVariant,
  runsWell,
} from './modelFilters';

function variant(id: string, patch: Partial<VariantView> = {}): VariantView {
  return {
    id,
    label: id,
    recommended: false,
    size: 1,
    memory_estimate_mb: 1000,
    files: 1,
    fit: { fit: 'fits', estimate_mb: 1000, budget_mb: 8000, offload_mb: 0, headroom: 0.8 },
    supported_here: true,
    state: 'not_installed',
    downloaded: 0,
    ...patch,
  };
}

function model(id: string, patch: Partial<ModelView> = {}): ModelView {
  return {
    id,
    name: id,
    publisher: 'p',
    modality: 'llm',
    engine: 'llama-cpp',
    description_pl: '',
    description_en: '',
    languages: ['en'],
    features: [],
    license: { name: 'MIT', url: '', commercial: 'yes', eu_allowed: true, gated: false },
    tags: [],
    custom: false,
    variants: [variant('q4')],
    default_variant: null,
    scores: null,
    ...patch,
  };
}

const big = { fit: 'wont_run', estimate_mb: 90000, budget_mb: 8000, offload_mb: 0, headroom: -10 } as const;

describe('filterModels', () => {
  const list = [
    model('bielik', { languages: ['pl'], tags: ['polski'] }),
    model('eu-banned', { license: { name: 'X', url: '', commercial: 'yes', eu_allowed: false, gated: false } }),
    model('nc', { modality: 'image', license: { name: 'R', url: '', commercial: 'no', eu_allowed: true, gated: false } }),
    model('huge', { variants: [variant('q8', { fit: big })] }),
    model('have', { variants: [variant('q4', { state: 'installed' })] }),
  ];

  it('hides EU-restricted models by default', () => {
    expect(filterModels(list, DEFAULT_FILTERS).map((m) => m.id)).not.toContain('eu-banned');
    expect(filterModels(list, { ...DEFAULT_FILTERS, euAllowed: false })).toHaveLength(5);
  });

  it('combines filters', () => {
    const ids = (f: Partial<typeof DEFAULT_FILTERS>) => filterModels(list, { ...DEFAULT_FILTERS, ...f }).map((m) => m.id);
    expect(ids({ polish: true })).toEqual(['bielik']);
    expect(ids({ commercial: true })).not.toContain('nc');
    expect(ids({ modality: 'image' })).toEqual(['nc']);
    expect(ids({ fitsHardware: true })).not.toContain('huge');
    expect(ids({ installed: 'installed' })).toEqual(['have']);
    expect(ids({ query: 'POLSKI' })).toEqual(['bielik']);
  });

  it('counts active filters', () => {
    expect(activeFilterCount(DEFAULT_FILTERS)).toBe(0);
    expect(activeFilterCount({ ...DEFAULT_FILTERS, polish: true, query: 'x' })).toBe(2);
  });
});

describe('variants', () => {
  it('prefers installed, then recommended fitting variants', () => {
    const m = model('m', {
      variants: [variant('a'), variant('b', { recommended: true }), variant('c', { state: 'installed' })],
    });
    expect(primaryVariant(m)?.id).toBe('c');
    m.variants[2]!.state = 'not_installed';
    expect(primaryVariant(m)?.id).toBe('b');
  });

  it('reports the best fit on this platform', () => {
    const m = model('m', { variants: [variant('a', { fit: big }), variant('b', { supported_here: false })] });
    expect(bestFit(m)).toBe('wont_run');
  });
});

describe('parseHfReference', () => {
  it('parses pasted URLs and short references', () => {
    expect(parseHfReference('https://huggingface.co/org/repo/blob/main/dir/My%20Model.gguf')).toEqual({
      repo: 'org/repo',
      path: 'dir/My Model.gguf',
    });
    expect(parseHfReference('org/repo/model-Q4_K_M.gguf')).toEqual({ repo: 'org/repo', path: 'model-Q4_K_M.gguf' });
    expect(parseHfReference('not a model')).toBeNull();
  });
});

describe('capabilityFits', () => {
  const sp = (speed: 'fast' | 'usable' | 'slow' | 'too_slow', reason?: 'needs_gpu' | 'not_enough_memory') => ({
    speed,
    value: 1,
    unit: 'tokens_per_second' as const,
    device: 'cpu' as const,
    reason,
  });

  it('summarises the best speed per capability', () => {
    const list = [
      model('llm-big', { variants: [variant('q8', { fit: big, speed: sp('too_slow', 'not_enough_memory') })] }),
      model('llm-small', { variants: [variant('q4', { speed: sp('usable') })] }),
      model('stt-large', { modality: 'stt', variants: [variant('q5', { speed: sp('slow') })] }),
      model('stt-base', { modality: 'stt', variants: [variant('q8', { speed: sp('usable') })] }),
      model('clone', { modality: 'tts', features: ['voice_cloning'], variants: [variant('v', { speed: sp('too_slow') })] }),
    ];
    const caps = capabilityFits(list);
    expect(caps.chat).toBe('usable');
    expect(caps.stt).toBe('usable');
    expect(caps.cloning).toBe('too_slow');
    expect(caps.tts).toBeNull();
  });

  it('reports a missing graphics card and missing memory', () => {
    const list = [
      model('img', { modality: 'image', variants: [variant('q4', { speed: sp('too_slow', 'needs_gpu') })] }),
      model('llm', { variants: [variant('q8', { fit: big, speed: sp('too_slow', 'not_enough_memory') })] }),
    ];
    const caps = capabilityFits(list);
    expect(caps.image).toBe('needs_gpu');
    expect(caps.chat).toBe('wont_run');
  });

  it('falls back to the memory fit without a speed estimate', () => {
    const caps = capabilityFits([model('a'), model('b', { modality: 'image', variants: [variant('x', { fit: big })] })]);
    expect(caps.chat).toBe('fast');
    expect(caps.image).toBe('wont_run');
  });
});

describe('runsWell', () => {
  it('needs memory and an acceptable speed', () => {
    expect(runsWell(variant('a'))).toBe(true);
    expect(runsWell(variant('b', { fit: big }))).toBe(false);
    expect(
      runsWell(variant('c', { speed: { speed: 'too_slow', value: 1, unit: 'realtime', device: 'cpu' } })),
    ).toBe(false);
    expect(runsWell(variant('d', { speed: { speed: 'slow', value: 1, unit: 'realtime', device: 'cpu' } }))).toBe(true);
  });
});

describe('sortModels', () => {
  const scored = (id: string, overall: number, prog: number | null, tps: number | null) =>
    model(id, {
      scores: {
        name: id,
        overall,
        rated: 10,
        tokens_per_second: tps,
        quantization: 'Q4_K_M',
        params_b: 9,
        categories: prog === null ? {} : { Programming: { avg: prog, n: 33 } },
      },
    });
  const list = [model('none-a'), scored('a', 3.5, 2.4, 80), model('none-b'), scored('b', 4.2, 3.8, 55), scored('c', 3.2, null, 120)];

  it('sorts by a benchmark category, untested models last', () => {
    expect(sortModels(list, 'cat:Programming').map((m) => m.id)).toEqual(['b', 'a', 'none-a', 'none-b', 'c']);
  });

  it('sorts by overall score and by speed', () => {
    expect(sortModels(list, 'overall').map((m) => m.id).slice(0, 3)).toEqual(['b', 'a', 'c']);
    expect(sortModels(list, 'speed').map((m) => m.id).slice(0, 3)).toEqual(['c', 'a', 'b']);
  });

  it('keeps catalogue order by default and validates keys', () => {
    expect(sortModels(list, 'default')).toBe(list);
    expect(isSortKey('cat:Polski')).toBe(true);
    expect(isSortKey('bogus')).toBe(false);
  });
});

describe('offeredCapabilities', () => {
  const model = (modality: string, features: string[] = []) => ({ modality, features, custom: false }) as never;

  it('lists only what the catalog has', () => {
    expect(offeredCapabilities([model('llm'), model('decision')])).toEqual(['chat']);
    expect(offeredCapabilities([model('llm'), model('image'), model('tts', ['voice_cloning'])])).toEqual(['chat', 'image', 'cloning']);
  });
});
