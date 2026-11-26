import { describe, expect, it } from 'vitest';
import { chats, titleOf, upsert, type PhoneChat } from './chats.svelte';

const chat = (id: string): PhoneChat => ({ id, title: id, updated: 0, messages: [{ role: 'user', content: id }] });

describe('the phone chat list', () => {
  it('names a chat after its first question', () => {
    expect(titleOf([{ role: 'user', content: '  Jak  ugotować\nrosół? ' }])).toBe('Jak ugotować rosół?');
    expect(titleOf([{ role: 'assistant', content: 'Cześć' }])).toBe('');
    expect(titleOf([{ role: 'user', content: 'x'.repeat(100) }])).toHaveLength(48);
  });

  it('puts the newest chat first and keeps the list short', () => {
    const list = upsert([chat('a'), chat('b')], chat('b'));
    expect(list.map((c) => c.id)).toEqual(['b', 'a']);
    expect(upsert([chat('a'), chat('b')], chat('c'), 2).map((c) => c.id)).toEqual(['c', 'a']);
  });

  it('saves, reopens and clears a chat', () => {
    chats.startNew();
    chats.messages = [{ role: 'user', content: 'Pierwsze pytanie' }];
    chats.save();
    const id = chats.currentId;
    expect(id).not.toBeNull();
    expect(chats.list[0]?.title).toBe('Pierwsze pytanie');

    chats.startNew();
    expect(chats.messages).toEqual([]);
    chats.open(id!);
    expect(chats.messages[0]?.content).toBe('Pierwsze pytanie');

    chats.clear();
    expect(chats.messages).toEqual([]);
    expect(chats.list.some((c) => c.id === id)).toBe(false);
  });

  it('does not keep an empty chat', () => {
    chats.startNew();
    chats.messages = [{ role: 'user', content: 'x' }, { role: 'assistant', content: '' }].slice(1) as typeof chats.messages;
    chats.save();
    expect(chats.currentId).toBeNull();
  });
});
