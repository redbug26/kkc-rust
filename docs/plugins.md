# Creating KKC Plugins

KKC loads Lua plugins at startup. Plugins can add:

- archive access, so a file can be entered like a directory;
- viewer extensions, selected from FileID mime types, to improve the internal viewer display.

User plugins are installed in the `data_dir()/plugins` directory from the `ProjectDirs` crate.
From KKC, this directory is available through `Options > Plugins > Open Dir`.

## Structure

A plugin is a directory containing a `plugin.lua` file:

```text
plugins/
  my_plugin/
    plugin.lua
    module.lua
```

The plugin directory is added to Lua's `package.path`, so `plugin.lua` can load local modules:

```lua
local helper = require("module")
```

Plugins can also be distributed as ZIP archives with the `.kkplug` extension.
When the user presses `Enter` on a `.kkplug` file in KKC, it is extracted into `data_dir()/plugins`.
The bundle must contain a `plugin.lua` at the plugin root:

```text
my_plugin.kkplug
  plugin.lua
  module.lua
```

or with a single wrapping root directory:

```text
my_plugin.kkplug
  my_plugin/
    plugin.lua
    module.lua
```

## The `kkc` Module

Plugins usually start with:

```lua
local kkc = require("kkc")
```

Available functions:

- `kkc.register_archive_plugin(table)`: registers an archive plugin.
- `kkc.register_viewer_plugin(table)`: registers a viewer plugin.
- `kkc.path_join(base, child)`: builds a path.
- `kkc.create_dir_all(path)`: creates a directory and its parents.
- `kkc.write_file(path, content)`: writes a file, creating parent directories when needed.

`sj.error(message)` is also available and raises a Lua error.

## Archive Plugins

An archive plugin lets KKC enter a file and extract its contents into a temporary directory.

Declaration:

```lua
kkc.register_archive_plugin({
    name = "simple_archive",
    version = "1.0.0",
    description = "Example archive plugin",
    mime_types = { "application/foo" },
    extract = function(path, destination)
        kkc.write_file(kkc.path_join(destination, "content.txt"), "Extracted from " .. path)
        return true
    end,
})
```

Fields:

- `name`: stable unique plugin identifier.
- `version`: optional semantic version string shown in `Options > Plugins`. Defaults to `0.0.0` for older plugins.
- `description`: text shown in `Options > Plugins`.
- `mime_types`: supported mime types as reported by FileID (`idf.rs`).
- `extensions`: legacy fallback for older plugins. Prefer `mime_types`.
- `extract(path, destination)`: extracts content into `destination`, then returns `true`.
- `add_files(path, files)`: optional. Allows copying local files into the archive. `files` is a Lua table of paths.
- `can_handle(path)`: optional. Some bundled plugins use this for their own checks, but KKC discovers archive support from `mime_types`.

Example with archive writing:

```lua
kkc.register_archive_plugin({
    name = "writable_archive",
    version = "1.0.0",
    description = "Example writable archive",
    mime_types = { "application/foo" },
    extract = function(path, destination)
        return true
    end,
    add_files = function(path, files)
        for _, source in ipairs(files) do
            -- Read source and modify path here.
        end
        return true
    end,
})
```

## Viewer Plugins

A viewer plugin can be selected in the internal viewer with `F4: Change Viewer`.

There are two forms:

- `render_line(path, mode, line)`: colors a line already decoded by the viewer.
- `render(path, mode, state, width)`: produces the whole document to display. This is useful for disk catalogs, indexes, formatted data, and other generated views.
- `handle_key(path, mode, key, state)`: optional. Handles keys while the plugin viewer is active and can update plugin state.

Viewer plugin fields:

- `name`: stable unique plugin identifier.
- `version`: optional semantic version string shown in `Options > Plugins`. Defaults to `0.0.0` for older plugins.
- `description`: text shown in `Options > Plugins` and `F4: Change Viewer`.
- `modes`: supported viewer modes, usually `{ "text" }`.
- `mime_types`: optional. FileID mime types for automatic viewer plugin selection.
- `extensions`: legacy fallback for older plugins. Prefer `mime_types`.

Currently useful modes:

- `text`
- `ansi`

Viewer plugins are available from `F4: Change Viewer > P. Plugins viewer`.
When a full-document plugin is active, KKC gives it the full viewer panel width and disables automatic wrapping for the plugin output.

### Coloring Lines

```lua
local kkc = require("kkc")

kkc.register_viewer_plugin({
    name = "keywords",
    version = "1.0.0",
    description = "Simple highlighting",
    modes = { "text" },
    render_line = function(path, mode, line)
        if mode ~= "text" then
            return nil
        end
        if line:match("^%s*#") then
            return {
                { text = line, fg = "green", bg = "black", bold = false },
            }
        end
        return {
            { text = line, fg = "white", bg = "black", bold = false },
        }
    end,
})
```

### Rendering a Complete Document

```lua
local kkc = require("kkc")

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

kkc.register_viewer_plugin({
    name = "index_viewer",
    version = "1.0.0",
    description = "Displays a custom index",
    modes = { "text" },
    mime_types = { "application/x-index" },
    render = function(path, mode, state, width)
        if mode ~= "text" then
            return nil
        end
        return {
            { span("Index for " .. path, "yellow", true) },
            { span("") },
            { span("Line 1", "white") },
            { span("Line 2", "cyan") },
        }
    end,
})
```

`render()` returns a table of lines. Each line is a table of spans.
Arguments:

- `path`: file being viewed.
- `mode`: current viewer mode, usually `text`.
- `state`: a Lua table containing string keys and string values previously returned by `handle_key`.
- `width`: current content width in terminal cells. Use this to keep generated tables inside the viewer.

A span is a table:

```lua
{ text = "text", fg = "white", bg = "black", bold = false }
```

The host clips plugin spans to the viewer width and replaces control characters with printable spacing before drawing. Plugins should still avoid returning control codes, raw carriage returns, or terminal escape sequences.

### Handling Keys And State

Full-document viewer plugins can react to keys. This is useful for sorting, changing a view mode, or expanding/collapsing generated content.

```lua
local kkc = require("kkc")

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

local function render(path, mode, state, width)
    if mode ~= "text" or not path:lower():match("%.foo$") then
        return nil
    end
    state = state or {}
    local view = state.view or "summary"
    return {
        {
            span("FOO", "yellow", true),
            span("  view: ", "gray"),
            span(view, "lightcyan"),
            span("  [v] switch view", "darkgray"),
        },
    }
end

local function handle_key(path, mode, key, state)
    if mode ~= "text" or not path:lower():match("%.foo$") then
        return nil
    end
    state = state or {}
    local view = state.view or "summary"
    if key == "char:v" then
        if view == "summary" then
            view = "details"
        else
            view = "summary"
        end
        return {
            consumed = true,
            state = { view = view },
        }
    end
    return {
        consumed = false,
        state = state,
    }
end

kkc.register_viewer_plugin({
    name = "foo_viewer",
    version = "1.0.0",
    description = "Example stateful viewer",
    modes = { "text" },
    render = render,
    handle_key = handle_key,
})
```

`handle_key()` result:

- `consumed`: `true` if KKC should not also process the key.
- `state`: string-key/string-value table saved in the `Viewer` and passed to the next `render()` and `handle_key()` calls.

Plugin-facing key strings:

- characters: `char:x`, for example `char:s`, `char:<`, `char:>`.
- arrows/navigation: `left`, `right`, `up`, `down`, `home`, `end`, `pgup`, `pgdown`.
- actions: `enter`, `tab`, `backtab`.
- function keys: `f1`, `f2`, ... using the terminal function-key number.

Ctrl-modified keys are reserved by KKC and are not sent to plugins.

Supported colors:

- `black`
- `red`
- `green`
- `yellow`
- `blue`
- `magenta`
- `cyan`
- `gray` or `grey`
- `darkgray` or `darkgrey`
- `lightred`
- `lightgreen`
- `lightyellow`
- `lightblue`
- `lightmagenta`
- `lightcyan`
- `white`

Tip: use `bg = "black"` for viewer plugins. KKC's viewer content area is black.

## Best Practices

- Return `nil` when the plugin does not want to handle the file or mode.
- Return only valid UTF-8 strings. If you read an old character set, convert it to Unicode before returning spans.
- Never write outside `destination` in `extract`.
- For archive plugins, group all chunks of a single file before writing it.
- For a viewer over a binary format or generated document, prefer `render(path, mode, state, width)` over `render_line`.
- For tabular viewers, use the `width` argument when computing column sizes.
- Normalize line endings before returning text. For example, strip trailing `\r` from CRLF files.
- Keep `handle_key()` state small and string-based; it is copied back into the Rust `Viewer`.
- Keep `name` stable: KKC uses it to select the viewer plugin.

## Bundled Examples

The plugins bundled in `assets/plugins` can be used as examples:

- `amstrad_dsk`: archive + Amstrad CPC DSK catalog viewer.
- `commodore_d64`: archive + Commodore 64 D64 directory viewer.
- `csv_viewer`: CSV table viewer with sortable columns.
- `eml_viewer`: EML/MIME message viewer.
- `html_viewer`: rendered HTML text viewer.
- `json_viewer`: pretty/tree JSON viewer.
- `markdown_viewer`: rendered Markdown viewer.
- `text_syntax`: line-by-line syntax highlighting viewer.
- `xml_viewer`: structured XML viewer with syntax highlighting.
- `pdf_file`: read-only PDF exploration as an archive.
- `lha_lzh`: LHA/LZH exploration in Lua.
