import { describe, expect, it } from 'vitest';
import { AUTO_MODEL_PARAMS } from './ipc';
import { GPU_LAYERS_ALL, gpuFromSlider, gpuSliderValue, isAuto, styleOf, withStyle, withValue } from './modelParams';

describe('response style', () => {
  it('maps Basic styles to a temperature and back', () => {
    expect(withStyle(AUTO_MODEL_PARAMS, 'precise')).toEqual({ ...AUTO_MODEL_PARAMS, temperature: 0.2 });
    expect(withStyle({ ...AUTO_MODEL_PARAMS, top_k: 40 }, 'balanced')).toEqual(AUTO_MODEL_PARAMS);
    expect(styleOf(withStyle(AUTO_MODEL_PARAMS, 'creative'))).toBe('creative');
    expect(styleOf(AUTO_MODEL_PARAMS)).toBe('balanced');
  });

  it('is unknown for values set by hand in Expert', () => {
    expect(styleOf({ ...AUTO_MODEL_PARAMS, temperature: 0.6 })).toBeNull();
    expect(styleOf({ ...AUTO_MODEL_PARAMS, temperature: 0.2, top_p: 0.9 })).toBeNull();
  });
});

describe('withValue', () => {
  it('clamps, rounds and clears', () => {
    expect(withValue(AUTO_MODEL_PARAMS, 'temperature', 3)).toMatchObject({ temperature: 2 });
    expect(withValue(AUTO_MODEL_PARAMS, 'top_p', 0.9000001)).toMatchObject({ top_p: 0.9 });
    expect(withValue(AUTO_MODEL_PARAMS, 'top_k', 40.4)).toMatchObject({ top_k: 40 });
    expect(withValue({ ...AUTO_MODEL_PARAMS, top_k: 40 }, 'top_k', null)).toEqual(AUTO_MODEL_PARAMS);
    expect(isAuto(withValue(AUTO_MODEL_PARAMS, 'temperature', Number.NaN))).toBe(true);
  });
});

describe('GPU layers', () => {
  it('a Basic style keeps the GPU setting', () => {
    expect(withStyle({ ...AUTO_MODEL_PARAMS, gpu_layers: 0 }, 'creative').gpu_layers).toBe(0);
    expect(styleOf({ ...AUTO_MODEL_PARAMS, gpu_layers: 10 })).toBe('balanced');
    expect(isAuto({ ...AUTO_MODEL_PARAMS, gpu_layers: 0 })).toBe(false);
  });

  it('maps the slider top to all layers', () => {
    expect(gpuFromSlider(36, 36)).toBe(GPU_LAYERS_ALL);
    expect(gpuFromSlider(12.4, 36)).toBe(12);
    expect(gpuFromSlider(0, 36)).toBe(0);
    expect(gpuSliderValue(GPU_LAYERS_ALL, 36)).toBe(36);
    expect(gpuSliderValue(null, 36)).toBe(36);
    expect(gpuSliderValue(8, 36)).toBe(8);
  });
});
