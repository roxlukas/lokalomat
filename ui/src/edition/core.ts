// What both editions have: the free Lokalomat (ADR 0035).
import Hammer from '@lucide/svelte/icons/hammer';
import Boxes from '@lucide/svelte/icons/boxes';
import Cpu from '@lucide/svelte/icons/cpu';
import Server from '@lucide/svelte/icons/server';
import Settings from '@lucide/svelte/icons/settings';
import ChatCircle from 'phosphor-svelte/lib/ChatCircleIcon';
import Scales from 'phosphor-svelte/lib/ScalesIcon';
import { registerInspectorSection, registerView, registerWorkshopMode } from '../lib/edition/registry';

registerView({ id: 'workshop', labelKey: 'nav.workshop', icon: Hammer, order: 10, load: () => import('../views/WorkshopView.svelte') });
registerView({ id: 'models', labelKey: 'nav.models', icon: Boxes, order: 50, load: () => import('../views/ModelsView.svelte') });
registerView({ id: 'engines', labelKey: 'nav.engines', icon: Cpu, order: 60, expert: true, load: () => import('../views/EnginesView.svelte') });
registerView({ id: 'server', labelKey: 'nav.server', icon: Server, order: 80, expert: true, load: () => import('../views/ServerView.svelte') });
registerView({ id: 'settings', labelKey: 'nav.settings', icon: Settings, order: 90, bottom: true, load: () => import('../views/SettingsView.svelte') });

// Workshop modes of the free edition: chat and (Expert) decisions.
registerWorkshopMode({ id: 'chat', order: 10, icon: ChatCircle, example: true, load: () => import('../views/workshop/ChatPanel.svelte') });
registerWorkshopMode({ id: 'decision', order: 60, icon: Scales, expert: true, load: () => import('../views/workshop/DecisionPanel.svelte') });

// Inspector sections (ADR 0036): the chat model's parameters.
registerInspectorSection({ mode: 'chat', load: () => import('../views/workshop/ChatInspector.svelte') });
