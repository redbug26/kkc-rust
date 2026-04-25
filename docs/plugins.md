# Creating KKC Plugins

KKC loads Lua plugins at startup. Plugins can add:

- archive access, so a file can be entered like a directory;
- viewer extensions, to improve the internal viewer display.

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
    description = "Example archive plugin",
    extensions = { "foo" },
    extract = function(path, destination)
        kkc.write_file(kkc.path_join(destination, "content.txt"), "Extracted from " .. path)
        return true
    end,
})
```

Fields:

- `name`: stable unique plugin identifier.
- `description`: text shown in `Options > Plugins`.
- `extensions`: supported extensions, without leading dots.
- `extract(path, destination)`: extracts content into `destination`, then returns `true`.
- `add_files(path, files)`: optional. Allows copying local files into the archive. `files` is a Lua table of paths.

Example with archive writing:

```lua
kkc.register_archive_plugin({
    name = "writable_archive",
    description = "Example writable archive",
    extensions = { "foo" },
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
- `render(path, mode)`: produces the whole document to display. This is useful for disk catalogs, indexes, and other generated views.

Currently useful modes:

- `text`
- `ansi`

### Coloring Lines

```lua
local kkc = require("kkc")

kkc.register_viewer_plugin({
    name = "keywords",
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
    description = "Displays a custom index",
    modes = { "text" },
    render = function(path, mode)
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
A span is a table:

```lua
{ text = "text", fg = "white", bg = "black", bold = false }
```

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
- For a viewer over a binary format, prefer `render(path, mode)` over `render_line`.
- Keep `name` stable: KKC uses it to select the viewer plugin.

## Bundled Examples

The plugins bundled in `assets/plugins` can be used as examples:

- `amstrad_dsk`: archive + Amstrad CPC DSK catalog viewer.
- `commodore_d64`: archive + Commodore 64 D64 directory viewer.
- `text_syntax`: line-by-line syntax highlighting viewer.
- `pdf_file`: read-only PDF exploration as an archive.
- `lha_lzh`: LHA/LZH exploration in Lua.
