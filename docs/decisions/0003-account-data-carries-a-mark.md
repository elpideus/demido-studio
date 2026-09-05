# 0003. Account data carries a mark, and the endpoint decides

Status: accepted
Decided: [#26](https://github.com/elpideus/demido-studio/issues/26)

## Decision

An account carries a sensitivity class, `personal` or `operational`, defaulting
to `personal`. A tool result from a `personal` account is **marked**, and the
mark propagates to every event derived from it, model turns and compaction
summaries included.

The mark is a property of an event. The gate is a property of an assembly,
recomputed at every dispatch from what is in the window. Whether marked data may
reach a given endpoint is decided by one field on the provider, `training`, with
values `bars`, `does-not` and `unknown`. Local weights always pass, `unknown`
asks once per provider, keyless sources are refused and not asked.

A refusal has no override and no confirmation dialog.

## Consequences

Easy: auditing. Every input to the gate is a fact already written to the session
log, so "did any Google data ever reach a remote endpoint" is a query rather than
an investigation, and the answer is recomputable long after the fact.

Easy: adding a provider or an account type. Both are a value, not a code path.

Hard: nothing may read a payload to decide whether it is sensitive. A search
returning no hits is still marked. That over-approximation is the price of a rule
that cannot be wrong quietly, and it means a chat that once touched mail keeps
the mark until the events leave the window.

Foreclosed: consent as an escape. The user is the registered party and may
consent to a transfer, but cannot consent on Google's behalf to training, nor for
a stranger reading the prompt at an anonymous endpoint. This is the one place in
the design where Demido overrules the user, and it rests on irreversibility
rather than on caution.
