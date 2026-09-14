# 0007. A chat outranks its character

Status: accepted
Decided: [#32](https://github.com/elpideus/demido-studio/issues/32)

## Decision

The settings ladder is **global, then model, then character, then chat**,
following-or-overridden. The chat is the last word.

This applies to the offered set as much as to temperature, and the offered set is
the reason it was worth deciding: an override there **replaces** rather than
merges, so whichever layer wins wins completely.

## Consequences

Easy: the composer's tool picker always works. A user looking at one
conversation can turn tools on for it without editing a persona shared by every
other chat that uses it, which is what the control appears to promise.

Easy: nothing changes in code. `demido-settings/src/stack.rs` already ordered the
layers this way, with a test. What changed is two rule files that described it
backwards.

Hard: a character cannot **guarantee** it is tools-free. It sets a strong
default, and a chat can overrule it. The conversational persona that
[#20](https://github.com/elpideus/demido-studio/issues/20) leaned on when it
deleted Chat mode still works, because opening a chat on that character still
opens it with no tools, but it is a default rather than a promise.

Foreclosed: a persona that is a safety boundary. Anything that must hold
regardless of what a chat asks for belongs in the agent mode matrix, which is
where permissions are gated and which a chat cannot widen.
