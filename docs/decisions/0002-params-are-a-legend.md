# 0002. A command's `params` is a legend, not a signature

Status: accepted
Decided: [wayfinder #25](https://github.com/elpideus/demido-studio/issues/25)

## Decision

A slash command's typed line is never split. `/name` takes a free-text tail,
which reaches the prompt file through the single `$ARGUMENTS` placeholder as it
did in v2.

`params` has three readers and gates none of them. The user gets a static legend
beside the composer. The model gets the declarations appended to the expansion,
under fixed host text telling it to take each value from what was typed or from
the conversation, to ask for any required parameter it cannot find, and not to
guess. The host does nothing with the array: no validation, no refusal, no
default.

`required` is therefore addressed to the model alone.

## Consequences

Makes it easy: an author declares parameters without buying a form, and a
command with no `params` behaves exactly as it did in v2. The Navigator keeps
one row per command, because nothing is collected and nothing expands.

Makes it hard: a small model can guess a required value instead of asking for
one, and nothing in v0.1 notices. That is the accepted price of a free line.

Forecloses: positional parsing, at any layer. The brief's own example carries
`"since last January"` as one value of three tokens, so a splitter is wrong
before an author has written their second command.
