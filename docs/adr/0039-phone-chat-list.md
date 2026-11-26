# ADR 0039: Chat list on the phone, red delete buttons, yellow warnings

- Status: accepted
- Date: 2026-11-26

## Context

The owner asked for GUI changes:

- Workshop on the computer: the chat list only in chat mode.
- Phone: a chat list in a drawer on the left with a button that opens it, and a
  red trash button that clears the chat.
- Computer: warnings in yellow (they were blue `note-info` and easy to miss).
- Delete, remove and clear buttons in red everywhere.

Until now the phone kept no chats: a conversation was gone when the app closed.

## Decision

- **Phone chats are kept on the phone** (`ui/src/mobile/chats.svelte.ts`,
  localStorage key `lokalsi.mobile.chats`), at most 50, oldest dropped first. One
  list serves the chat on the phone (ADR 0038) and the chat with the computer; an
  opened chat continues with whichever is selected now. The computer's chat
  sessions are not synced to the phone (that would need new gateway endpoints and
  would not work for the phone-only chat).
- `MobileChatBar.svelte`: menu button (drawer from the left: new chat, open,
  delete), the model in the middle, red trash on the right. Clearing and deleting
  take two taps (the button turns into a filled red "Wyczyścić?"/"Usunąć?" for 4 s).
- Workshop: `SessionDrawer` and its toggle render only when `workshop.mode === 'chat'`.
- `base.css` now owns the danger styles: `.btn.danger` / `.icon-btn.danger`
  (outlined red) for the first tap, `.btn-danger` (filled red) for the final
  confirmation. Local copies in views were removed.
- "You need a model / engine" notes in the Workshop and Engines use `note-warn`;
  `note-warn` got a stronger border and a 4 px yellow bar on the left. Progress
  notes ("Preparing the engine…") stay `note-info`.

## Consequences

- Chat texts now stay on the phone in the WebView's storage (never sent anywhere);
  removing the app or its data removes them.
- Not yet tested on the Pixel (owner away from the lab on 2026-11-26).
