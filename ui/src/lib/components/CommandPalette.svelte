<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import { router } from '../router.svelte';
  import { resolvedTheme, settings } from '../settings.svelte';
  import { logToApp } from '../errors';
  import { errorMessage } from '../ipc';
  import { visibleViews } from '../views';

  let { open = $bindable(false) }: { open?: boolean } = $props();

  interface Command {
    id: string;
    label: string;
    run: () => void | Promise<void>;
  }

  let dialog: HTMLDialogElement | undefined = $state();
  let query = $state('');
  let selected = $state(0);

  const commands = $derived<Command[]>([
    ...visibleViews(settings.value.expert).map((v) => ({
      id: `go:${v.id}`,
      label: t('palette.goto', { view: t(v.labelKey) }),
      run: () => router.go(v.id),
    })),
    {
      id: 'theme',
      label: t('palette.toggleTheme'),
      run: () => settings.update({ theme: resolvedTheme(settings.value.theme) === 'dark' ? 'light' : 'dark' }),
    },
    {
      id: 'language',
      label: t('palette.switchLanguage'),
      run: () => settings.update({ language: settings.value.language === 'pl' ? 'en' : 'pl' }),
    },
  ]);

  const filtered = $derived(
    commands.filter((c) => c.label.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())),
  );

  const activeId = $derived.by(() => {
    const command = filtered[selected];
    return command ? `palette-${command.id}` : undefined;
  });

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      query = '';
      selected = 0;
      dialog.showModal();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  });

  $effect(() => {
    // keep the selection inside the filtered list
    if (selected >= filtered.length) selected = Math.max(0, filtered.length - 1);
  });

  async function execute(command: Command | undefined) {
    if (!command) return;
    open = false;
    try {
      await command.run();
    } catch (e) {
      logToApp('error', `palette command ${command.id} failed: ${errorMessage(e)}`);
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      selected = filtered.length ? (selected + 1) % filtered.length : 0;
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      selected = filtered.length ? (selected - 1 + filtered.length) % filtered.length : 0;
    } else if (event.key === 'Enter') {
      event.preventDefault();
      execute(filtered[selected]);
    }
  }
</script>

<dialog
  bind:this={dialog}
  class="palette"
  aria-label={t('palette.label')}
  onclose={() => (open = false)}
  onclick={(e) => e.target === dialog && (open = false)}
>
  <div class="inner">
    <input
      class="query"
      type="text"
      bind:value={query}
      placeholder={t('palette.placeholder')}
      aria-label={t('palette.label')}
      aria-controls="palette-list"
      aria-activedescendant={activeId}
      role="combobox"
      aria-expanded="true"
      onkeydown={onKeydown}
    />
    <ul id="palette-list" role="listbox" aria-label={t('palette.label')}>
      {#each filtered as command, i (command.id)}
        <li
          id={`palette-${command.id}`}
          role="option"
          aria-selected={i === selected}
          class:selected={i === selected}
          onpointermove={() => (selected = i)}
          onclick={() => execute(command)}
          onkeydown={() => undefined}
        >
          {command.label}
        </li>
      {:else}
        <li class="empty" role="presentation">{t('palette.empty')}</li>
      {/each}
    </ul>
    <p class="hint">{t('palette.hint')}</p>
  </div>
</dialog>

<style>
  .palette {
    width: min(560px, calc(100vw - 32px));
    margin: 12vh auto auto;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  .palette::backdrop {
    background: var(--overlay);
  }

  .inner {
    display: flex;
    flex-direction: column;
  }

  .query {
    padding: var(--space-4);
    border: 0;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    background: transparent;
    font-size: var(--text-l);
  }

  .query:focus-visible {
    outline: none;
    box-shadow: none;
  }

  ul {
    max-height: 50vh;
    margin: 0;
    padding: var(--space-2);
    overflow-y: auto;
    list-style: none;
  }

  li {
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-s);
    cursor: pointer;
  }

  li.selected {
    background: var(--surface-3);
  }

  li.empty {
    color: var(--text-muted);
    cursor: default;
  }

  .hint {
    padding: var(--space-2) var(--space-4);
    border-top: 1px solid var(--border);
    color: var(--text-faint);
    font-size: var(--text-xs);
  }
</style>
