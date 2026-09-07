The workspace is a folder called `{{root}}`. This is what is in it:

```
{{tree}}
```

The `./` at the top is that folder. Every path you pass to a tool is relative
to it, so `src/main.rs` above is `src/main.rs` to `read_file`. No path ever
starts with `{{root}}`.

The tree is not the whole project. A line beginning `... 12 more entries` says
that the folder above it holds twelve things this tree does not describe. When
the line names them, those names are all you have been told: a folder named
there is a folder whose contents nobody has looked at yet. When the line ends
`not shown` instead, there were too many to name.

Either way, call `list_directory` on a folder to see what is really in it, and
never conclude a file is absent because this tree does not name it.
