/**
 * Blocks the git commands an agent session has no business running here.
 *
 * A PreToolUse hook: it reads the tool call on stdin and exits 2 to refuse it.
 * History rewriting, force pushing and branch deletion are Stefan's calls to
 * make, at a terminal, with the repository in front of him. An agent that wants
 * one of these asks for it instead.
 *
 * Set up on wayfinder ticket #16, from the git-guardrails-claude-code skill.
 * The skill ships a bash script that parses the call with jq; this is the same
 * rules in node, because jq is not installed on this machine and a guardrail
 * that silently passes everything is worse than none.
 *
 * Test:
 *   echo '{"tool_input":{"command":"git push origin main"}}' | node .claude/hooks/block-dangerous-git.mjs
 */

const BLOCKED = [
  [/\bgit\s+push\b/, 'pushing is a person\'s decision, not a session\'s'],
  [/\bpush\s+--force|\bpush\s+-f\b/, 'force pushing rewrites what other clones already have'],
  [/\bgit\s+reset\s+--hard\b/, 'a hard reset throws away work that was never committed'],
  [/\bgit\s+clean\s+-[a-z]*f/, 'git clean deletes untracked files with no undo'],
  [/\bgit\s+branch\s+-D\b/, 'deleting a branch unreviewed loses whatever only it held'],
  [/\bgit\s+checkout\s+\.(\s|$)/, 'checkout . discards every uncommitted change at once'],
  [/\bgit\s+restore\s+\.(\s|$)/, 'restore . discards every uncommitted change at once'],
  [/\bgit\s+rebase\b.*\s-i\b|\bgit\s+rebase\s+-i\b/, 'an interactive rebase cannot run without a terminal'],
  [/\bgit\s+commit\b[^|;&]*--amend\b/, 'amending rewrites a commit that may already be signed and pushed'],
  [/\bgit\s+filter-branch\b|\bfilter-repo\b/, 'history rewriting is never an incidental step'],
]

let input = ''
process.stdin.setEncoding('utf8')
process.stdin.on('data', (chunk) => (input += chunk))
process.stdin.on('end', () => {
  // The command if the call parses, and the whole payload if it does not. A
  // malformed call is still matched rather than waved through.
  let command = input
  try {
    command = JSON.parse(input)?.tool_input?.command ?? input
  } catch {
    /* keep the raw payload */
  }

  for (const [pattern, why] of BLOCKED) {
    if (pattern.test(command)) {
      process.stderr.write(
        `BLOCKED: ${pattern.source}\n` +
          `${why}.\n` +
          `You do not have authority over this command in this repository. Ask Stefan to run it.\n`,
      )
      process.exit(2)
    }
  }
  process.exit(0)
})
