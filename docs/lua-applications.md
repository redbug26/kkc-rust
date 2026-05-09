# Lua Applications In KKC

KKC can run terminal applications written in Lua through an internal runtime.
This runtime is designed for interactive terminal apps and small games.

## Location

A Lua app is a directory containing:

- `app.toml`
- `app.lua` (or a custom main file defined in `app.toml`)

Bundled examples:

- `assets/applications/tetris`
- `assets/applications/ascii`
- `assets/applications/calculator`
- `assets/applications/calendar`

Official starter template:

- `assets/applications/template_lua_app`

At startup, KKC copies bundled apps into `data_dir()/applications`.

## Manifest

`app.toml` format:

```toml
[app]
id = "tetris"
name = "Tetris"
version = "0.1.0"
description = "Terminal Tetris"
main = "app.lua"
fps = 20
```

Fields:

- `id`: unique application id (used in launcher command).
- `name`: display name.
- `version`: app version string.
- `description`: short description.
- `main`: entry Lua script relative to the app directory. Default: `app.lua`.
- `fps`: target frame rate. Default 30, clamped by host runtime.

## Launch Command

Use an opener command with this format:

- `kkc-lua-app:<id>`

Example:

- `kkc-lua-app:tetris`
- `kkc-lua-app:ascii %f`

KKC also exposes a dedicated internal launcher from the menu:

- `Tools > Run Lua app`

When a file is opened through an association, `%f` and other opener placeholders are expanded before passing args to the Lua app.

## Lua Runtime API

The app script should return a table:

```lua
local app = {}

function app.init(ctx) end
function app.update(dt) end
function app.draw() end
function app.keypressed(key) end
function app.resize(w, h) end
function app.mousepressed(button, x, y) end
function app.mousereleased(button, x, y) end
function app.mousedragged(button, x, y) end
function app.mousemoved(x, y) end
function app.mousewheel(dx, dy, x, y) end

return app
```

All callbacks are optional.

### `require("kkc")`

Returns a module with:

- `quit()`: request app exit.
- `time()`: seconds elapsed since app start.
- `args()`: command-line args table.
- metadata fields: `id`, `name`, `version`.

### `require("kkc-graphics")`

Terminal draw API (1-based coordinates):

- `size()` -> `width, height`
- `clear(ch)`
- `put(x, y, ch)`
- `text(x, y, text)`
- `box(x, y, w, h, ch)`

### `require("kkc-key")`

Key constants:

- `LEFT`, `RIGHT`, `UP`, `DOWN`, `SPACE`, `ENTER`, `ESC`

`keypressed()` receives names like:

- `left`, `right`, `up`, `down`, `space`, `enter`, `esc`
- `char:x` for character keys
- `tab`, `backspace`

### `require("kkc-rand")`

Pseudo-random helpers:

- `seed(number)`
- `int(min, max)`
- `float()`

### `require("kkc-fs")`

Simple file helpers for app scripts:

- `exists(path)`
- `is_dir(path)`
- `read_text(path)`
- `write_text(path, content)`
- `list_dir(path?)`
- `mkdir_all(path)`
- `join(a, b)`

Relative paths are resolved from the application directory.

### `require("kkc-audio")`

Terminal audio helpers:

- `beep()` (terminal bell)

### `require("kkc-mouse")`

Mouse constants:

- Buttons: `LEFT`, `RIGHT`, `MIDDLE`
- Kinds: `UP`, `DOWN`, `DRAG`, `MOVE`
- Wheel: `SCROLL_UP`, `SCROLL_DOWN`, `SCROLL_LEFT`, `SCROLL_RIGHT`

Coordinates passed to callbacks are 1-based (same convention as `kkc-graphics`).

## Store Distribution

Lua apps can be distributed through the same store plugin flow:

- package an app directory with `app.toml` and `app.lua`
- publish it as a store plugin item location
- KKC accepts this payload as a valid install target

This enables community-distributed Lua apps without requiring external binaries.

## Starter Pack And CI Lint

Use `assets/applications/template_lua_app` as a base project.

It includes:

- `app.toml` + `src/app.lua` starter
- `.luacheckrc`
- `.github/workflows/lua-lint.yml` for GitHub Actions
