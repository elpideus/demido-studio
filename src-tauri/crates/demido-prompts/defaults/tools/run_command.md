Run a shell command and read what it printed. It starts in the workspace root unless you give a directory. Use this for building, testing, searching and version control. A command that exits with a non-zero status counts as failed. Very long output is cut, and the result says how much was left out.

## command

The command line to run, as you would type it in a terminal.

## cwd

Directory to start in, relative to the workspace root. Omit to start in the root.

## timeout_seconds

Give up after this long. Omit for 60 seconds. At most 600.
