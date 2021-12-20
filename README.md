# Pore

> pore (verb) \
> to read or study attentively

Pore is a command line [full-text
search](https://en.wikipedia.org/wiki/Full-text_search) tool powered by
[tantivy](https://github.com/quickwit-inc/tantivy).

**When would I use this instead of grep or ripgrep?**

If you can express what you're looking for as a regular expression or exact text
string, use ripgrep. If you want something more like a Google search, use pore.

```
USAGE:
    pore [OPTIONS] [--] [ARGS]

ARGS:
    <query>


    <dir>


OPTIONS:
        --color <color>
            This flag controls when to use colors. The default setting is auto, which will try to
            guess when to use colors.
               The possible values for this flag are:

                   never    Colors will never be used.
                   auto     Auto-detect if the terminal supports colors (default).
                   always   Colors will always be used regardless of where output is sent.
                   ansi     Like 'always', but emits ANSI escapes (even in a Windows console).

        --delete
            Delete the cached index files for the directory (if any)

        --files
            Print out the files that would be searched (do not perform the search)

    -g, --glob <glob>...
            Include or exclude files and directories for searching that match the given glob. This
            always overrides any other ignore logic. Multiple glob flags may be used. Precede a glob
            with a ! to exclude it.

        --glob-case-insensitive
            Patterns passed to --glob and --oglob will be matched in a case-insentive way.

    -h, --help
            Print help information

        --hidden
            Search hidden files and directories

    -i, --index <index>
            Use the specified index for querying (must be specified in the config file)

        --in-memory
            Do not store the text index on disk (will have to rebuild every time)

        --indexes
            print out the indexes that would be used (do not perform the search)

    -j, --threads <threads>
            The approximate number of threads to use. A value of 0 (which is the default) will
            choose the thread count using heuristics.

        --json
            Print the results as json

    -l, --files-with-matches
            Print out the files that match the search (not the matching lines).

    -L, --follow
            Follow symbolic links

        --language <language>
            The language to use for parsing files

        --limit <limit>
            Maximum number of files to return

        --no-follow
            Don't follow symbolic links (overrides --follow)

        --no-hidden
            Ignore hidden files and directories (overrides --hidden)

        --no-ignore
            Don't respect .gitignore files

        --no-memory
            Force the index to be saved to disk (overrides --in-memory)

        --no-update
            Do not update the index before performing the query

        --oglob <oglob>...
            Only search files that match this glob. Files that do not match any of these globs will
            be ignored.

        --rebuild
            Force rebuild the index before searching

        --threshold <threshold>
            Minimum score threshold for results

    -u, --update
            Update the index before searching (the default)

    -V, --version
            Print version information
```

## Config

The config file is located at `${XDG_CONFIG_HOME}/pore.toml` (default
`$HOME/.config/pore.toml`). An example can be found at
[pore.example.toml](https://github.com/stevearc/pore/blob/master/pore-bin/pore.example.toml).
The format is:

```toml
# These are the global arguments that are used by default
limit = 10

# You can add an index with different customizations.
# These are used by passing --index=NAME
[index-NAME]
    oglob = "*.md,*.rst,*.txt"

# You can add additional customizations for a specific directory
[local-myproject]
    # Be sure to specify the path
    path = "/path/to/myproject"

    # These options will override the global ones
    language = "Arabic"

    # Local projects can specify their own indexes.
    # They are also used by passing --index=OTHER_INDEX
    [local-myproject.OTHER_INDEX]
        limit = 20
```
