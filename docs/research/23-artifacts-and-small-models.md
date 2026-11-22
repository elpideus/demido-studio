# 23. Do small models give a file to keep when they should?

Written 2026-11-22, directly. Brief 4.24 asks for artifacts: files a model makes and sends in the session,
which it can update, whose earlier versions the person can see, and which can be downloaded; "Claude Desktop
has this feature". Anthropic's help page (read on 22 November 2026) says Claude makes one when what it shares
is "significant and self-contained, typically over 15 lines", likely to be edited, reused or referred back to.
Demido's are built (DECISIONS F10 to F14); this is whether the test models use them.

How sure each line is: **Tried** (on the owner's computer with `core/dev/artifact-trial.py`, 22 November 2026,
llama.cpp b12359, decisions at temperature 0 and the rest with each maker's settings, thinking off,
reading from the internet off). It drives a real
Demido server on a scratch data folder, so the tools are listed and asked about exactly as in a session.
Small numbers: direction, not a benchmark.

## The trial

Each request in a new session. Four that should give a file: a one-page HTML site for a bakery, a cover
letter of about 250 words, a Python script that renames files, a packing list as a Markdown checklist. Then a
change to the bakery's page, asked after it ("Change the hours to 8 to 14"): a new version of the same file.
Four that should not: the capital of Portugal, a mutex in two sentences, 17 times 23, hello in French. And one
in a session with a folder where the person asks for a file there (`write_file`'s work, not an artifact's).
The four that should not, and the folder one, were asked with artifacts on and off.

## The first build: the text inside the decision

The tool was `give_file(name, text)`: the model chose it, and wrote the file, in the one JSON reply that
chooses a tool.

| Model | Gave a file, of 4 | The change, as a new version | Asked for it in a way that could not be used |
|---|---|---|---|
| Qwen3 0.6B | 0 | no | 1 |
| Qwen3 1.7B | 0 | named the file `give_file.md` | 0 |
| Granite 3.3 2B | 0 | no | 0 |
| Qwen3 4B | 1 (the letter) | no | 3 (the page, twice; the change) |

1. **The decision is held to 200 tokens, and a page does not fit in it.** Every time Qwen3 4B chose the tool
   for the page, its reply was cut off mid-text and refused as unusable. The same holds for `write_file`,
   which carries the whole of a file's text inside the decision: no model can write more than about 150
   words into a file with it (IDEAS 137).
2. **The smaller models answer in the reply instead**, which is not wrong: the person gets the page or the
   letter, only not as a file of its own.
3. **The tool takes nothing from the other choices.** The four requests that should not give a file, and the
   folder one, went the same with artifacts on and off; no model gave a file for any of them. The differences
   there are the ones these models always have (Granite reaches for the calculator for a capital city, with
   artifacts on or off).

## The second build: name it, then write it

`give_file(name)`: the decision names the file, and its text is asked for after it, as ordinary text with
room for a few pages (4,000 tokens), the way an answer is written (note 13's "decide, then answer").

| Model | Gave a file, of 4 | The change |
|---|---|---|
| Qwen3 0.6B | 2, both named `give_file.md` (for the page, a second file followed, named after its heading) | `give_file.md` again, then refused |
| Qwen3 1.7B | 2, named `give_file.md` | `give_file.md`, then refused |
| Granite 3.3 2B | 1 (`Sol_Bakery_Hours.html`) | answered in the reply |
| Qwen3 4B | 1 (the letter) | refused |

4. **Models chose the tool more often once it no longer asked them to write the file inside a JSON reply**
   (6 of 16, against 1), but the two Qwen models **put the tool's name where the file's belongs**: the
   argument was called `name`, and they gave the tool's. Qwen3 4B's choice for the page was still refused.

## The third build: `file_name`, and a cut-off decision mended

The argument became `file_name`. A decision cut off because the model went on to write the file into it is
taken for what it plainly is (the tool and the name), since the text is asked for next in any case; a file
named after the tool is refused, and asked for again.

| Model | Gave a file, of 4 | The change | Asked to add a file to the folder |
|---|---|---|---|
| Qwen3 0.6B | 0 | answered in the reply | **gave an artifact** (with artifacts off: `add_to_file`) |
| Qwen3 1.7B | 1 (`index.html`) | **version 2 of `index.html`** | `write_file` |
| Granite 3.3 2B | 0 | gave `bakery_hours.html`, then a new file, `updated_bakery_hours.html` | `write_file` |
| Qwen3 4B | 2 (`cover_letter.md`, `rename_txt_to_md.py`) | refused | **gave an artifact** (with artifacts off: `write_file`) |

5. **Every name was a good one**, and Qwen3 1.7B made a true second version.
6. **Two models gave an artifact when the person asked for a file in the folder.** The tool did not say where
   its file goes, and `write_file`'s words ("Creates a new file") do not say either.
7. **Granite made a new file instead of a new version**: it was not shown the files it had given.
8. **Qwen3 4B's page was still refused**, as "not a JSON object": not the shape the mend reads.

## The fourth and fifth builds: where the file goes, what was given, and arguments first

Three changes. In a session with a folder the tool says its file is "not put in the folder (write_file puts a
file in the folder)". With the tools, the model is shown the files it already gave ("Files you gave the person
in this session: bakery.html. The same name again gives a new version."). And the mend of a cut-off decision
reads `file_name` even when the tool is not named: the models write `"args"` before `"tool"`, so a reply cut off
in the text never reaches the tool's name. The fourth run showed that; the fifth has all three.

| Model | Gave a file, of 4 (fifth run) | The change (fifth run) | Asked to add a file to the folder (fourth run) |
|---|---|---|---|
| Qwen3 0.6B | 0 | answered in the reply | **gave an artifact** (off: `add_to_file`) |
| Qwen3 1.7B | 1 (`index.html`) | **version 2 of `index.html`** | `write_file` |
| Granite 3.3 2B | 0 | gave `bakery_hours.html`, then a second file under a new name | `write_file` |
| Qwen3 4B | **3** (`bakery.html`, `cover_letter.md`, `rename_txt_to_md.py`) | **version 2 of `bakery.html`** | gave an artifact, then also wrote the file |

The four requests that should give nothing gave nothing, in every run, on and off. The packing list went to
`plan` for Qwen3 4B every time: a list of things to pack reads to it as steps.

## What follows

9. **Name it, then write it.** A file's text is never asked for inside the decision. The same is true of
   `write_file` today, and it fails the same way: in the fourth run, with artifacts off, Qwen3 4B's notes for
   the folder were cut off and refused (IDEAS 137).
10. **Mend what is plain.** A cut-off decision that names a file is the tool that has `file_name`; a file
    named after the tool is no name.
11. **Show what was given.** Qwen3 4B and 1.7B made true new versions once they saw the names; Granite did
    not.
12. **By default, not in a session with a folder.** There the person's files are on disk and `write_file`
    puts them there; the smallest model still gave an artifact when asked for a file in the folder, even when
    the tool said where its file goes. In sessions without a folder, nothing was lost by offering it.
13. **The person does not wait for the model.** Two of the four models seldom or never choose the tool; any
    answer can be kept as a file by a press (DECISIONS F12).

What this does not show: models between 4B and 12B, which the owner's card would run, and anything about the
quality of the files beyond their names (they were not graded).

## The same limit for files in the folder

`write_file` carries a file's whole text inside the decision, which is held to 200 tokens. Four requests in a
session with a folder (three ideas for a blog post, a README of about 200 words, a renaming script, a line
added to `todo.txt`), with artifacts off, on the build before and after the change (DECISIONS T24): the same
tools and files for every model both times. Qwen3 4B wrote all four (the README 139 words before, which fit,
and 179 after; its script took one more try after); the smaller models answered the longer ones in the reply,
as they always have. A file's text is written with each maker's settings, so its length varies. Then a README of about 400 words, Qwen3 4B, three times each:

| Build | Written | What happened |
|---|---|---|
| Before | 0 of 3 | cut off and refused every time. The reply began `{"content": "## To-Do App…`: the text first, before the file or the tool was named, so there was nothing plain to mend |
| After, first try | 3 of 3, but to a file named `README.md',` | the second try, held to `write_file` without the text, named the file with the JSON's own quote and comma caught in it |
| After | **3 of 3**, 211 to 239 words, as `README.md` | |

14. **A second try names the file only.** When a cut-off reply plainly began a file's text (`content` is
    `write_file`'s), the second try is held to that tool without the text, and told so; the text is asked for
    next.
15. **The quotes and comma of the JSON are not part of a path.**
