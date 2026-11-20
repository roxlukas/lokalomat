// What the Workshop frame and its mode panels share (ADR 0035). The frame
// (`views/WorkshopView.svelte`) draws the tabs, the output area and the command
// bar; the panel of the open mode (`views/workshop/*Panel.svelte`) puts its
// pieces into the frame through `workshop.attach`.
import type { Component, Snippet } from 'svelte';
import { engines } from './chat.svelte';
import { readLocal } from './storage';

export const MODE_KEY = 'lokalsi.workshop-mode';

/** A decision pending from another place: "use again" (Gallery, voices) for one mode. */
export interface Reuse {
  mode: string;
  params: Record<string, unknown>;
}

/** What a panel gives the frame. Getters keep it reactive without re-attaching. */
export interface PanelView {
  /** Cards or messages for the output area. */
  output?: Snippet;
  /** Extra items in the top bar, next to the mode tabs. */
  top?: Snippet;
  /** Chips, notes and fields above the input row. */
  controls?: Snippet;
  /** Replaces the default text input row (the transcription drop button). */
  input?: Snippet;
  /** Something to show instead of the empty-screen examples. */
  readonly hasOutput: boolean;
  /** Changes when the newest output should scroll into view. */
  readonly scrollKey: string | number;
  readonly canSend: boolean;
  send(): void | Promise<void>;
  readonly placeholder: string;
  readonly maxlength?: number;
  readonly submitLabel: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  readonly submitIcon: Component<any>;
  /** While an answer streams: a stop button instead of the send button. */
  readonly stop?: { label: string; run: () => void };
  /** Audio dropped on the window (Tauri gives real paths). */
  readonly audioDrop?: { show: boolean; handles: boolean; run: (path: string) => void };
}

class Workshop {
  mode = $state<string>(readLocal<string>(MODE_KEY, 'chat'));
  prompt = $state('');
  error = $state('');
  /** Keep the newest output in view unless the user scrolled up. */
  stick = $state(true);
  dropActive = $state(false);
  reuse = $state.raw<Reuse | null>(null);
  /** A recording dropped while another mode was open; the transcription panel takes it. */
  pendingAudio = $state<string | null>(null);
  view = $state.raw<(PanelView & { owner: string }) | null>(null);

  attach(owner: string, view: PanelView): void {
    this.view = Object.assign(view, { owner });
  }

  detach(owner: string): void {
    if (this.view?.owner === owner) this.view = null;
  }

  focusInput(): void {
    setTimeout(() => document.getElementById('workshop-input')?.focus(), 0);
  }
}

export const workshop = new Workshop();

/** The "model is installed" state of the engine a mode needs. */
export function engineStatus(engineId: string | undefined): {
  unavailable: boolean;
  install: [number, number] | null;
} {
  const e = engineId ? (engines.list.find((x) => x.id === engineId) ?? null) : null;
  return {
    unavailable: Boolean(e && !e.installed && e.builds.length === 0),
    install: e?.state === 'installing' ? e.install_progress : null,
  };
}
