# 0013. A sub-agent is a scope on one log

Status: accepted
Decided: [#62](https://github.com/elpideus/demido-studio/issues/62)

Brief B07: "recorded in an append-only session log"

## Decision

**Every event carries the agent that produced it, and a sub-agent records into
its parent's journal rather than one of its own.** The main session is
`AgentId::MAIN`; a delegation opens a child named after the call that opened it,
and `agent/delegated` is the parent's record of opening it.

The field is on every event from the commit that adds it, the main session's
included, because a scope that only children carry is one the monitor cannot
ask about the whole run.

A child is a second `Session` over the same journal: its own agent, its own
depth, and **its own bookkeeping**. It writes out every wording and every
offered set its assemblies name, even the ones its parent already wrote, so its
assembly rebuilds out of its own events.

## Consequences

Easy: the monitor's agent scope is a filter over one stream, and selecting an
agent is a query rather than a second log to open, a second file to keep in step
and a second thing that can be lost.

Easy: a delegation is ordered against the turn that asked for it. Two logs would
have two clocks and no answer to *did this come back before the step boundary*,
which is the one property the asynchronous path is asserted on
([#66](https://github.com/elpideus/demido-studio/issues/66)).

Easy: one journal handle, so one sequence counter. Two handles over one file
each number from where they opened it, and the second line claiming position
nine is a log that cannot be replayed at all.

Hard: a turn number is no longer unique on the log, because a sub-agent runs the
agent loop again and counts its own exchanges from one. Anything that reads the
log by turn reads it through an agent, and the default is the conversation.

Hard: a child repeats its parent's paragraph and tool wordings in its own half
of the log. That is the cost of a rebuild that does not depend on the parent's
half being there beside it, and it is paid once per hash per agent rather than
per turn.

Foreclosed: a sub-agent whose transcript the user asked not to keep while the
parent's is kept. One journal is one retention decision, and splitting it later
means splitting the events that are already in it.
