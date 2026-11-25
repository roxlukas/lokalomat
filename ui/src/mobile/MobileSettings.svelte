<script lang="ts">
  import Cpu from 'phosphor-svelte/lib/CpuIcon';
  import Desktop from 'phosphor-svelte/lib/DesktopIcon';
  import Plus from 'phosphor-svelte/lib/PlusIcon';
  import Trash from 'phosphor-svelte/lib/TrashIcon';
  import { t } from '../lib/i18n/index.svelte';
  import { api, describe, type Computers } from './api';

  /** Settings: the paired computers (add, forget) and the models on this phone. */
  let {
    computers,
    onClose,
    onAdd,
    onModels,
    onChanged,
    canUsePhone,
  }: {
    computers: Computers;
    onClose: () => void;
    onAdd: () => void;
    onModels: () => void;
    /** A computer was forgotten: the app reloads its state. */
    onChanged: () => void | Promise<void>;
    canUsePhone: boolean;
  } = $props();

  let asking = $state<{ id: string; name: string } | null>(null);
  let error = $state('');

  async function forget() {
    if (!asking) return;
    const { id } = asking;
    asking = null;
    error = '';
    try {
      await api.computerRemove(id);
      await onChanged();
    } catch (e) {
      error = describe(e);
    }
  }
</script>

<div class="settings" role="dialog" aria-modal="true" aria-labelledby="settings-title">
  <header>
    <h2 id="settings-title">{t('mobile.settings.title')}</h2>
    <button type="button" class="btn" onclick={onClose}>{t('common.close')}</button>
  </header>

  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}

  <section aria-labelledby="computers-title">
    <h3 id="computers-title">{t('mobile.settings.computers')}</h3>
    {#each computers.list as c (c.id)}
      <div class="row">
        <Desktop size={22} aria-hidden="true" />
        <div class="what">
          <strong>{c.name}</strong>
          <span class="muted small">{c.address}</span>
        </div>
        <button
          type="button"
          class="icon-btn danger"
          aria-label={t('mobile.forget')}
          title={t('mobile.forget')}
          onclick={() => (asking = { id: c.id, name: c.name })}
        >
          <Trash size={20} aria-hidden="true" />
        </button>
      </div>
    {:else}
      <p class="muted small">{t('mobile.settings.noComputers')}</p>
    {/each}
    <button type="button" class="btn big" onclick={onAdd}>
      <Plus size={20} aria-hidden="true" />{t('mobile.settings.addComputer')}
    </button>
  </section>

  {#if canUsePhone}
    <section aria-labelledby="models-title">
      <h3 id="models-title">{t('mobile.settings.models')}</h3>
      <button type="button" class="btn big" onclick={onModels}>
        <Cpu size={20} aria-hidden="true" />{t('mobile.settings.manageModels')}
      </button>
    </section>
  {/if}
</div>

{#if asking}
  <div class="sheet-backdrop">
    <div class="sheet" role="alertdialog" aria-modal="true" aria-labelledby="forget-title" aria-describedby="forget-body">
      <h2 id="forget-title">{t('mobile.forgetConfirm.title', { name: asking.name })}</h2>
      <p id="forget-body" class="muted">{t('mobile.forgetConfirm.body')}</p>
      <div class="sheet-actions">
        <button type="button" class="btn" onclick={() => (asking = null)}>{t('common.cancel')}</button>
        <button type="button" class="btn btn-danger" onclick={forget}>{t('mobile.forgetConfirm.confirm')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .settings {
    position: fixed;
    inset: 0;
    z-index: 25;
    display: grid;
    align-content: start;
    gap: var(--space-4);
    padding: max(var(--space-8), env(safe-area-inset-top, 0px)) var(--space-4)
      max(var(--space-6), env(safe-area-inset-bottom, 0px));
    overflow-y: auto;
    background: var(--bg);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    font-size: var(--text-l);
  }

  h3 {
    margin-bottom: var(--space-2);
    font-size: var(--text-m, 1rem);
  }

  section {
    display: grid;
    gap: var(--space-2);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--surface);
  }

  .what {
    display: grid;
    flex: 1;
    min-width: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .big {
    justify-content: center;
    width: 100%;
    min-height: 52px;
  }

  .icon-btn {
    min-width: 48px;
    min-height: 48px;
  }

  .icon-btn.danger {
    color: var(--error);
  }

  .sheet-backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    align-items: flex-end;
    background: color-mix(in srgb, var(--bg) 60%, transparent);
  }

  .sheet {
    display: grid;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-5) var(--space-4) max(var(--space-6), env(safe-area-inset-bottom, 0px));
    border-top: 1px solid var(--border);
    border-radius: var(--radius-l) var(--radius-l) 0 0;
    background: var(--surface);
  }

  .sheet h2 {
    font-size: var(--text-l);
  }

  .sheet-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .btn-danger {
    border-color: var(--error);
    background: var(--error);
    color: var(--bg);
    font-weight: 700;
  }
</style>
