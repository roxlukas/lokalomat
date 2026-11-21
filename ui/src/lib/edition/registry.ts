// What an edition adds to the app (ADR 0035). The entry module of an edition
// (`src/edition/*.ts`, chosen at build time by the `@edition` alias) registers
// its views, start-up work and extra UI here. Pro code is referenced only from
// the Pro entry, through dynamic imports, so the free build does not contain it.
import type { Component } from 'svelte';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyComponent = Component<any>;
export type Loader = () => Promise<{ default: AnyComponent }>;

export interface ViewDef {
  id: string;
  labelKey: string;
  icon: AnyComponent;
  /** Position in the rail, small first. */
  order: number;
  /** Pinned to the bottom of the rail. */
  bottom?: boolean;
  /** Shown only in Expert mode (PRD §15: hide what is too hard for most users). */
  expert?: boolean;
  load: Loader;
}

/** A mode of the Workshop (chat, image, speech…); its panel is loaded when it opens. */
export interface WorkshopModeDef {
  id: string;
  /** Position among the tabs, small first. */
  order: number;
  /** Shown only in Expert mode. */
  expert?: boolean;
  /** Has an example on the empty Workshop screen (`workshop.examples.<id>`). */
  example?: boolean;
  load: Loader;
}

/** A tab of the Server view added by an edition. */
export interface ServerTab {
  id: string;
  /** Translation key of the tab's label. */
  labelKey: string;
  load: Loader;
}

/** Browser-preview stand-in for an edition's IPC commands; answers with the `UNSUPPORTED` symbol of `mock.ts` when it does not know the command. */
export type MockHandler = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

/** Places where an edition can add components (rendered by `SlotHost`). */
export type SlotName = 'overlay' | 'tool-call' | 'chat-bar' | 'about';

const views: ViewDef[] = [];
const slots: Record<SlotName, Loader[]> = { overlay: [], 'tool-call': [], 'chat-bar': [], about: [] };
const workshopModes: WorkshopModeDef[] = [];
const serverTabs: ServerTab[] = [];
const startups: (() => Promise<void> | void)[] = [];
const blockers: (() => boolean)[] = [];
const mocks: MockHandler[] = [];

export function registerView(def: ViewDef): void {
  const i = views.findIndex((v) => v.id === def.id);
  if (i >= 0) views.splice(i, 1);
  views.push(def);
  views.sort((a, b) => a.order - b.order);
}

export function registeredViews(): readonly ViewDef[] {
  return views;
}

export function findView(id: string): ViewDef | undefined {
  return views.find((v) => v.id === id);
}

export function registerWorkshopMode(def: WorkshopModeDef): void {
  const i = workshopModes.findIndex((m) => m.id === def.id);
  if (i >= 0) workshopModes.splice(i, 1);
  workshopModes.push(def);
  workshopModes.sort((a, b) => a.order - b.order);
}

export function registeredWorkshopModes(): readonly WorkshopModeDef[] {
  return workshopModes;
}

export function registerServerTab(tab: ServerTab): void {
  serverTabs.push(tab);
}

export function registeredServerTabs(): readonly ServerTab[] {
  return serverTabs;
}

export function registerSlot(name: SlotName, load: Loader): void {
  slots[name].push(load);
}

export function slotItems(name: SlotName): readonly Loader[] {
  return slots[name];
}

/** Work to do when the app starts (loading stores, listening for events). */
export function registerStartup(fn: () => Promise<void> | void): void {
  startups.push(fn);
}

export async function runStartups(onError: (error: unknown) => void): Promise<void> {
  await Promise.all(
    startups.map(async (fn) => {
      try {
        await fn();
      } catch (e) {
        onError(e);
      }
    }),
  );
}

/** The page behind a dialog of an edition must not take focus while `block()` is true. */
export function registerBlocker(block: () => boolean): void {
  blockers.push(block);
}

export function blocked(): boolean {
  return blockers.some((b) => b());
}

export function registerMock(handler: MockHandler): void {
  mocks.push(handler);
}

export function registeredMocks(): readonly MockHandler[] {
  return mocks;
}

/** Test helper: forget everything registered. */
export function resetRegistry(): void {
  views.length = 0;
  startups.length = 0;
  blockers.length = 0;
  mocks.length = 0;
  serverTabs.length = 0;
  workshopModes.length = 0;
  for (const name of Object.keys(slots) as SlotName[]) slots[name].length = 0;
}
