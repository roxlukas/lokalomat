<script lang="ts">
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import { chat, effectiveChatModel } from '../../lib/chat.svelte';
  import { logToApp } from '../../lib/errors';
  import { describeError } from '../../lib/feedback';
  import { t } from '../../lib/i18n/index.svelte';
  import { AUTO_MODEL_PARAMS, errorMessage, ipc, type ModelParams } from '../../lib/ipc';
  import {
    GPU_LAYERS_ALL,
    gpuFromSlider,
    gpuSliderValue,
    isAuto,
    PARAM_SPECS,
    RESPONSE_STYLES,
    styleOf,
    withStyle,
    withValue,
    type ParamKey,
    type ResponseStyle,
  } from '../../lib/modelParams';
  import { settings } from '../../lib/settings.svelte';
  import { models } from '../../lib/stores.svelte';

  /** Inspector section of the chat mode (ADR 0036): defaults of the model the next message goes to. */
  const model = $derived(effectiveChatModel(models.list, chat.model));
  let params = $state<ModelParams>({ ...AUTO_MODEL_PARAMS });
  let error = $state('');
  /** Layers of the model (GGUF), the top of the GPU slider; null = unknown. */
  let layers = $state<number | null>(null);
  const style = $derived(styleOf(params));

  $effect(() => {
    const m = model;
    params = { ...AUTO_MODEL_PARAMS };
    layers = null;
    error = '';
    if (!m) return;
    let live = true;
    ipc
      .getModelLayers(m.modelId, m.variantId)
      .then((n) => {
        if (live) layers = n;
      })
      .catch((e) => logToApp('warn', `get_model_layers failed: ${errorMessage(e)}`));
    ipc
      .getModelParams(m.modelId, m.variantId)
      .then((p) => {
        if (live) params = p;
      })
      .catch((e) => {
        if (live) error = describeError(e);
      });
    return () => {
      live = false;
    };
  });

  async function save(next: ModelParams) {
    const m = model;
    if (!m) return;
    const previous = params;
    params = next;
    error = '';
    try {
      params = await ipc.setModelParams(m.modelId, m.variantId, next);
    } catch (e) {
      params = previous;
      error = describeError(e);
      logToApp('warn', `set_model_params failed: ${errorMessage(e)}`);
    }
  }

  function chooseStyle(s: ResponseStyle) {
    void save(withStyle(params, s));
  }

  function setAuto(key: ParamKey, auto: boolean) {
    const spec = PARAM_SPECS.find((p) => p.key === key);
    void save(withValue(params, key, auto ? null : (spec?.start ?? null)));
  }

  /** Short label of the GPU setting next to its name. */
  function gpuLabel(n: number, total: number | null): string {
    if (n === 0) return t('inspector.chat.gpuNone');
    if (n >= GPU_LAYERS_ALL || (total !== null && n >= total)) return t('inspector.chat.gpuAll');
    return total === null ? String(n) : t('inspector.chat.gpuOf', { n, total });
  }

  function format(key: ParamKey, value: number): string {
    return key === 'top_k' ? String(value) : value.toFixed(2);
  }
</script>

{#if model}
  <p class="model" title={model.label}>{model.label}</p>

  {#if settings.value.expert}
    <div class="params">
      {#each PARAM_SPECS as spec (spec.key)}
        {@const value = params[spec.key]}
        <div class="param">
          <div class="head">
            <label for={`param-${spec.key}`} title={t(`inspector.chat.${spec.key}Tip`)}>{t(`inspector.chat.${spec.key}`)}</label>
            {#if value !== null}<output for={`param-${spec.key}`}>{format(spec.key, value)}</output>{/if}
            <label class="auto">
              <input type="checkbox" checked={value === null} onchange={(e) => setAuto(spec.key, e.currentTarget.checked)} />
              {t('inspector.auto')}
            </label>
          </div>
          <input
            id={`param-${spec.key}`}
            type="range"
            min={spec.min}
            max={spec.max}
            step={spec.step}
            value={value ?? spec.start}
            disabled={value === null}
            oninput={(e) => (params = withValue(params, spec.key, Number(e.currentTarget.value)))}
            onchange={(e) => save(withValue(params, spec.key, Number(e.currentTarget.value)))}
          />
        </div>
      {/each}
      <div class="param">
        <div class="head">
          <label for="param-gpu" title={t('inspector.chat.gpuTip')}>{t('inspector.chat.gpu')}</label>
          <label class="auto">
            <input
              type="checkbox"
              checked={params.gpu_layers === null}
              onchange={(e) => save({ ...params, gpu_layers: e.currentTarget.checked ? null : GPU_LAYERS_ALL })}
            />
            {t('inspector.auto')}
          </label>
        </div>
        {#if layers !== null}
          <input
            id="param-gpu"
            type="range"
            min="0"
            max={layers}
            step="1"
            value={gpuSliderValue(params.gpu_layers, layers)}
            disabled={params.gpu_layers === null}
            oninput={(e) => (params = { ...params, gpu_layers: gpuFromSlider(Number(e.currentTarget.value), layers ?? 0) })}
            onchange={(e) => save({ ...params, gpu_layers: gpuFromSlider(Number(e.currentTarget.value), layers ?? 0) })}
          />
        {:else}
          <select
            id="param-gpu"
            value={String(params.gpu_layers ?? GPU_LAYERS_ALL)}
            disabled={params.gpu_layers === null}
            onchange={(e) => save({ ...params, gpu_layers: Number(e.currentTarget.value) })}
          >
            <option value={String(GPU_LAYERS_ALL)}>{t('inspector.chat.gpuAll')}</option>
            <option value="0">{t('inspector.chat.gpuNone')}</option>
          </select>
        {/if}
        {#if params.gpu_layers !== null}<output class="gpu-value" for="param-gpu">{gpuLabel(params.gpu_layers, layers)}</output>{/if}
        <p class="hint small">{t('inspector.chat.gpuHint')}</p>
      </div>
      <button type="button" class="btn reset" disabled={isAuto(params)} onclick={() => save({ ...AUTO_MODEL_PARAMS })}>
        <RotateCcw size={14} aria-hidden="true" />{t('inspector.reset')}
      </button>
    </div>
  {:else}
    <fieldset>
      <legend>{t('inspector.chat.style')}</legend>
      <div class="styles">
        {#each RESPONSE_STYLES as s (s)}
          <label class:checked={style === s}>
            <input type="radio" name="response-style" value={s} checked={style === s} onchange={() => chooseStyle(s)} />
            {t(`inspector.chat.styles.${s}`)}
          </label>
        {/each}
      </div>
      {#if style === null}<p class="hint">{t('inspector.chat.custom')}</p>{/if}
    </fieldset>
  {/if}
  <p class="hint">{t('inspector.chat.scope')}</p>
  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
{:else}
  <p class="hint">{t('inspector.chat.noModel')}</p>
{/if}

<style>
  .model {
    margin-top: var(--space-2);
    overflow: hidden;
    color: var(--text-muted);
    font-size: var(--text-s);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  fieldset {
    margin: var(--space-4) 0 0;
    padding: 0;
    border: 0;
  }

  legend {
    margin-bottom: var(--space-2);
    font-weight: 600;
  }

  .styles {
    display: flex;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--bg);
  }

  .styles label {
    flex: 1;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-s);
    font-size: var(--text-s);
    text-align: center;
    cursor: pointer;
  }

  .styles label.checked {
    background: var(--accent);
    color: var(--accent-contrast);
  }

  .styles input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .styles label:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .params {
    display: grid;
    gap: var(--space-4);
    margin-top: var(--space-4);
  }

  .head {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }

  .head > label:first-child {
    font-weight: 600;
    white-space: nowrap;
    cursor: help;
  }

  .gpu-value {
    display: block;
    font-family: var(--font-sans);
  }

  output {
    font-family: var(--font-mono);
    font-size: var(--text-s);
  }

  .auto {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: auto;
    color: var(--text-muted);
    font-size: var(--text-s);
  }

  input[type='range'] {
    width: 100%;
    accent-color: var(--accent);
  }

  input[type='range']:disabled {
    opacity: 0.4;
  }

  .reset {
    justify-self: start;
  }

  .hint.small {
    margin-top: var(--space-1);
    font-size: var(--text-xs);
  }

  .hint {
    margin-top: var(--space-3);
    color: var(--text-muted);
    font-size: var(--text-s);
  }

  .note {
    margin-top: var(--space-3);
  }
</style>
