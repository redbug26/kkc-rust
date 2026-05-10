# Lua Applications In KKC

KKC can run interactive terminal applications written in Lua through an
embedded runtime. Lua apps are rendered in a KKC-managed terminal window and
are suited for small tools, games, and file-aware utilities.

## Location

A Lua app is a directory containing:

- `app.toml`
- the main Lua script, usually `app.lua`

The main script may live elsewhere if `app.toml` sets `main`, for example
`main = "src/app.lua"`.

At startup, KKC installs bundled apps into `data_dir()/applications`. Runtime
lookup scans:

- `data_dir()/applications`
- `data_dir()/plugins`

Bundled examples:

- `assets/applications/ascii`
- `assets/applications/calculator`
- `assets/applications/calendar`
- `assets/applications/git_repo`
- `assets/applications/rockland`
- `assets/applications/snake`
- `assets/applications/tetris`

Official starter template:

- `assets/applications/template_lua_app`

## Manifest

`app.toml` format:

```toml
[app]
id = "tetris"
name = "Tetris"
version = "0.1.0"
description = "Terminal Tetris powered by KKC Lua app runtime"
main = "app.lua"
fps = 20
width = 46
height = 26
```

Fields:

- `id`: unique application id used by launcher commands.
- `name`: display name. Defaults to `id`.
- `version`: app version string. Defaults to `0.1.0`.
- `description`: short description. Defaults to an empty string.
- `main`: entry Lua script relative to the app directory. Defaults to `app.lua`.
- `fps`: target frame rate. Defaults to 30 and is clamped to 10..120.
- `width`: requested floating-window width, including border. Defaults to 40.
- `height`: requested floating-window height, including border. Defaults to 20.

The content area passed to Lua is the window size minus the KKC border.
Press `F5` while an app is running to toggle between the requested floating
window size and full-screen mode.

## Launch Command

Use an opener command with this format:

- `kkc-lua-app:<id>`

Examples:

- `kkc-lua-app:tetris`
- `kkc-lua-app:git_repo %f`

KKC also exposes a dedicated internal launcher from the menu:

- `Tools > Run Lua app`

When a file is opened through an association, `%f` and other opener
placeholders are expanded before passing args to the Lua app.

## App Shape

The main Lua script must return a table with callbacks, or set a global `app`
table. Returning a table is preferred.

```lua
local app = {}

function app.init(ctx) end
function app.resize(w, h) end
function app.update(dt) end
function app.draw() end

function app.keydown(key) end
function app.keypressed(key) end
function app.keyup(key) end

function app.mousepressed(button, x, y) end
function app.mousereleased(button, x, y) end
function app.mousedragged(button, x, y) end
function app.mousemoved(x, y) end
function app.mousewheel(dx, dy, x, y) end

function app.focuslost() end
function app.focusgained() end

function app.shortcuts()
    return { "Enter:Start", "Esc:Quit" }
end

return app
```

All callbacks are optional.

`init(ctx)` receives:

- `ctx.width`, `ctx.height`: initial drawable content area.
- `ctx.id`, `ctx.name`, `ctx.version`, `ctx.description`: manifest metadata.
- `ctx.args`: launcher arguments as a 1-based Lua table.

`resize(w, h)` is called when the app content area changes, including initial
window setup and `F5` zoom changes.

`update(dt)` receives elapsed seconds since the previous frame.

`shortcuts()` may return strings in `Key:Label` format. KKC renders them in
the native footer shortcut bar.

## Keyboard

`Ctrl-C` is handled by the host as a global quit key. Other keys are delivered
to Lua when recognized.

Key callback behavior:

- `keydown(key)`: first physical press when detectable.
- `keypressed(key)`: press and repeat events.
- `keyup(key)`: release event on terminals that support enhanced keyboard events.

Terminals without release-event support use a timeout-based held-key fallback
for `kkc-key.is_down`.

Recognized key names include:

- `left`, `right`, `up`, `down`
- `space`, `enter`, `esc`
- `backspace`, `delete`, `tab`
- `pageup`, `pagedown`, `home`, `end`, `insert`
- `f1`, `f2`, etc.
- `char:x` for character keys

## Runtime Modules

### `require("kkc")`

General app/runtime helpers:

- `quit()`: request app exit.
- `time()` -> seconds elapsed since app start.
- `args()` -> launcher args table.
- `cwd`: launch working directory as a string.
- `get_cwd()` -> launch working directory.
- metadata fields: `id`, `name`, `version`.

### `require("kkc-graphics")`

Terminal draw API. Coordinates are 1-based and target the drawable content
area.

- `size()` -> `width, height`
- `clear(ch?)`: fill the buffer with `ch`, default space.
- `put(x, y, ch)`: draw the first character of `ch`.
- `text(x, y, text)`: draw text at column `x`, row `y`.
- `print(row, col, text)`: row-first convenience alias for `text(col, row, text)`.
- `box(x, y, w, h, ch?)`: fill a rectangle with `ch`, default space.
- `color(fg, bg)`: set current colors as `0xRRGGBB`.
- `set_fg(fg)`: set foreground, keeping background.
- `set_bg(bg)`: set background, keeping foreground.
- `reset()`: reset colors to white on black.

Drawing outside the content area is clipped.

### `require("kkc-key")`

Key constants and held-key state:

- Constants: `LEFT`, `RIGHT`, `UP`, `DOWN`, `SPACE`, `ENTER`, `ESC`
- `HAS_RELEASE_EVENTS`: whether the terminal reports key release events.
- `is_down(name)` -> boolean for currently held keys.

`is_down` accepts the same key names delivered to callbacks.

### `require("kkc-mouse")`

Mouse constants:

- Buttons: `LEFT`, `RIGHT`, `MIDDLE`
- Kinds: `UP`, `DOWN`, `DRAG`, `MOVE`
- Wheel: `SCROLL_UP`, `SCROLL_DOWN`, `SCROLL_LEFT`, `SCROLL_RIGHT`

Mouse callbacks receive 1-based terminal coordinates and wheel deltas:

- scroll up: `mousewheel(0, 1, x, y)`
- scroll down: `mousewheel(0, -1, x, y)`
- scroll left: `mousewheel(-1, 0, x, y)`
- scroll right: `mousewheel(1, 0, x, y)`

### `require("kkc-rand")`

Lightweight pseudo-random helpers:

- `seed(number)`
- `int(min, max)`: inclusive range.
- `float()`: floating-point value in `[0, 1)`.

### `require("kkc-fs")`

File helpers for app scripts:

- `exists(path)`
- `is_dir(path)`
- `read_text(path)`
- `write_text(path, content)`
- `list_dir(path?)`
- `mkdir_all(path)`
- `join(a, b)`

Relative paths are resolved from the application directory. Absolute paths are
used as supplied.

### `require("kkc-shell")`

Shell facade for terminal apps:

- `run(program, args?, cwd?)` -> result table
- `cwd()` -> default launch working directory

`args` is a Lua sequence table. `run` executes with `stdin` closed and
`GIT_TERMINAL_PROMPT=0`. If `cwd` is omitted, the app launch working directory
is used.

The result table contains:

- `ok`: boolean exit success.
- `code`: process exit code, or `-1`.
- `stdout`: captured stdout string.
- `stderr`: captured stderr string.

### `require("kkc-audio")`

Terminal audio helpers:

- `beep()`: terminal bell.
- `play_audio(path)`: play a MOD/XM/S3M/WAV/FLAC/MP3 file and return `{ name, format, songs }`.
- `stop_audio()`: stop the current audio file.
- `is_audio_playing()`: return whether an audio file is active.

## Store Distribution

Lua apps can be distributed through the same store plugin flow:

- package an app directory with `app.toml` and its Lua files
- publish it as a store plugin item location
- KKC accepts this payload as a valid install target

This enables community-distributed Lua apps without requiring external
binaries.

## Starter Pack And CI Lint

Use `assets/applications/template_lua_app` as a base project.

It includes:

- `app.toml` + `src/app.lua` starter
- `.luacheckrc`
- `.github/workflows/lua-lint.yml` for GitHub Actions
