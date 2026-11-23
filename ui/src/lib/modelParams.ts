// Chat model parameters in the Inspector (ADR 0036). `null` means Auto: the
// engine's or model's own default.
import type { ModelParams } from './ipc';

/** The sampling values (the GPU setting has its own control). */
export type ParamKey = 'temperature' | 'top_p' | 'top_k';

/** `gpu_layers` meaning every layer on the GPU (`GPU_LAYERS_ALL` in `lokalsi_core::model_params`). */
export const GPU_LAYERS_ALL = 999;

/** The Basic choice: one control instead of three numbers. */
export type ResponseStyle = 'precise' | 'balanced' | 'creative';

export const RESPONSE_STYLES: readonly ResponseStyle[] = ['precise', 'balanced', 'creative'];

/** Temperature of each style; balanced keeps Auto. */
export const STYLE_TEMPERATURE: Record<ResponseStyle, number | null> = {
  precise: 0.2,
  balanced: null,
  creative: 1.0,
};

export interface ParamSpec {
  key: ParamKey;
  min: number;
  max: number;
  step: number;
  /** Value shown when the user switches Auto off (llama.cpp's own default). */
  start: number;
}

export const PARAM_SPECS: readonly ParamSpec[] = [
  { key: 'temperature', min: 0, max: 2, step: 0.05, start: 0.8 },
  { key: 'top_p', min: 0, max: 1, step: 0.05, start: 0.95 },
  { key: 'top_k', min: 0, max: 200, step: 1, start: 40 },
];

/** The Basic style these values match; `null` when they were set by hand in Expert. */
export function styleOf(p: ModelParams): ResponseStyle | null {
  if (p.top_p !== null || p.top_k !== null) return null;
  return RESPONSE_STYLES.find((s) => STYLE_TEMPERATURE[s] === p.temperature) ?? null;
}

/** Values for a Basic style: the temperature, the other sampling values back to Auto; the GPU setting stays. */
export function withStyle(p: ModelParams, style: ResponseStyle): ModelParams {
  return { ...p, temperature: STYLE_TEMPERATURE[style], top_p: null, top_k: null };
}

/** `p` with one value changed, clamped to its range and rounded to its step. */
export function withValue(p: ModelParams, key: ParamKey, value: number | null): ModelParams {
  if (value === null || !Number.isFinite(value)) return { ...p, [key]: null };
  const spec = PARAM_SPECS.find((s) => s.key === key);
  if (!spec) return p;
  const clamped = Math.min(spec.max, Math.max(spec.min, value));
  const rounded = key === 'top_k' ? Math.round(clamped) : Math.round(clamped * 100) / 100;
  return { ...p, [key]: rounded };
}

export function isAuto(p: ModelParams): boolean {
  return p.temperature === null && p.top_p === null && p.top_k === null && p.gpu_layers === null;
}

/** Slider position for `gpu_layers` on a model with `total` layers (all = `total`). */
export function gpuSliderValue(gpuLayers: number | null, total: number): number {
  return gpuLayers === null ? total : Math.min(gpuLayers, total);
}

/** `gpu_layers` for a slider position: the top means all layers, including the output layer. */
export function gpuFromSlider(value: number, total: number): number {
  const v = Math.max(0, Math.round(value));
  return v >= total ? GPU_LAYERS_ALL : v;
}
