Find every line in the workspace that contains a phrase. Give plain text, not a pattern: it is matched literally and case-insensitively. Answers with paths and line numbers, which read_file takes as from_line. Use this before reading files when you do not know which file holds something.

## text

The text to find. Matched literally, ignoring case.

## path

Directory to search, relative to the workspace root. Omit to search all of it.

## from

Number of the first match to return, as numbered in a previous search for the same text. Omit to start at the beginning.
