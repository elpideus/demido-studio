# 0011. The window draws the log, never the draft it accumulated

Status: accepted
Decided: [#43](https://github.com/elpideus/demido-studio/issues/43)

Brief B07: "recorded in an append-only session log"

## Decision

**A transcript is a projection of the session log, computed on demand, in every
process that draws one.** `demido_chat::Chat` holds no messages;
`Chat::history` is a scan of the events. The window holds no messages either:
`web/src/chat/chat.ts` reads the transcript through `chat_transcript` when the
desk mounts and again when a turn ends, and never appends to it.

**What the window accumulates while an answer streams is a draft, and the draft
is thrown away.** Tokens arrive over a Tauri event into a buffer outside React
(`web/src/chat/stream.ts`), which is drawn as the last bubble while the turn
runs and cleared when it ends. The bubble that replaces it is the log's own
copy, read back. The draft is never promoted, even though it holds the same
characters.

## Consequences

Easy: closing the window and opening it again draws the conversation with the
same read that drew it the first time. Nothing is restored, because nothing was
kept anywhere else, and there is no restore path to be wrong.

Easy: a stopped turn shows its partial answer without a special case. The log
recorded what arrived before the cancel, so reading the log back shows it.

Easy: the export and the Session Monitor are two more projections of the same
events rather than two more readers of a chat table.

Hard: one extra IPC round trip per turn, at the moment a turn ends. It buys the
property that the window cannot come to disagree with the log about what was
said, which is the claim the product makes about itself.

Hard: the streaming bubble and the recorded bubble are drawn by different code,
so a difference between them is possible and would be visible as a flicker at
the end of a turn. That is the failure this shape makes visible rather than the
one it hides: the alternative hides it forever.

Foreclosed: an optimistic transcript the window owns and reconciles. v2 kept a
message list beside its log, and two stores over one conversation is two stores
that can eventually disagree about it.
