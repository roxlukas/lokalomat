import { describe, expect, it } from 'vitest';
import { isSendKey, sendKeyAria, sendKeyHint, type SendKeyEvent } from './sendKey';

function key(over: Partial<SendKeyEvent> = {}): SendKeyEvent {
  return { key: 'Enter', ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, isComposing: false, ...over };
}

describe('isSendKey', () => {
  it('ctrl_enter: plain Enter adds a new line, Ctrl/⌘+Enter sends', () => {
    expect(isSendKey(key(), 'ctrl_enter')).toBe(false);
    expect(isSendKey(key({ shiftKey: true }), 'ctrl_enter')).toBe(false);
    expect(isSendKey(key({ ctrlKey: true }), 'ctrl_enter')).toBe(true);
    expect(isSendKey(key({ metaKey: true }), 'ctrl_enter')).toBe(true);
  });

  it('enter: plain Enter sends, Shift+Enter adds a new line, Ctrl+Enter still sends', () => {
    expect(isSendKey(key(), 'enter')).toBe(true);
    expect(isSendKey(key({ shiftKey: true }), 'enter')).toBe(false);
    expect(isSendKey(key({ altKey: true }), 'enter')).toBe(false);
    expect(isSendKey(key({ ctrlKey: true }), 'enter')).toBe(true);
  });

  it('never sends while an input method is composing or for other keys', () => {
    expect(isSendKey(key({ isComposing: true }), 'enter')).toBe(false);
    expect(isSendKey(key({ isComposing: true, ctrlKey: true }), 'ctrl_enter')).toBe(false);
    expect(isSendKey(key({ key: 'a', ctrlKey: true }), 'ctrl_enter')).toBe(false);
  });
});

describe('hints', () => {
  it('follow the scheme', () => {
    expect(sendKeyHint('ctrl_enter')).toBe('Ctrl+Enter');
    expect(sendKeyHint('enter')).toBe('Enter');
    expect(sendKeyAria('ctrl_enter')).toBe('Control+Enter');
    expect(sendKeyAria('enter')).toBe('Enter Control+Enter');
  });
});
