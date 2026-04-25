
# KKC — Ketchup Killers Commander

A classic dual-panel file manager inspired by Norton Commander, written in Rust with a terminal UI.

---

## History

The story of KKC starts in late **1996**. The author (RedBug, from the *Ketchup Killers* demo group) was 21, in his second year of IT studies, and a daily Norton Commander user with a long list of complaints about it. The solution was obvious: write his own.

The **first version (v0.1) shipped in early 1997**, targeting **MS-DOS** and compiled with Watcom C++. From that first release it already handled **ZIP, RAR, ARJ and LHA** archives, and could identify over **100 file formats** by their binary header via a custom library called IDF.

What set it apart from Norton Commander at the time:

- Custom ASCII character redefinition and a distinctive **beige colour scheme** that gave it its own personality
- **Automatic file viewer selection** based on file header recognition (not just extension)
- An internal **HTML viewer** (used by the built-in help system)
- A **macro editor** — all dialogs were built with a compilable macro language (used to produce the voting disk for Wired'98)
- **Long filename support under Windows 95**, almost unique among DOS programs in 1997
- A **BBS mode**: KKC could switch to ANSI mode and accept connections over a serial port

New versions rolled out over 18 months, typically released at **demoparties**: Saturne'97, LTP2, Wired'97, Mekka/Symposium'98… It was distributed on several BBS networks and is still available on the [Metropoli BBS](http://ftp.mpoli.fi/software/DOS/COMPRESS).

By **mid-1998**, MS-DOS held only 3.8% market share. The author moved to Windows, then to web development. A Linux port was attempted (the codebase had already migrated from Watcom to DJGPP), but too much code was tightly coupled to MS-DOS internals. **KKC was put to rest.**

The complete versioned DOS source code is preserved at **[redbug26/kkc-dos](https://github.com/redbug26/kkc-dos)**.

Nearly three decades later, this Rust rewrite brings KKC back — same spirit, modern stack.

---

## Features

- **Dual-panel interface** — classic commander-style layout with synchronized navigation
- **Internal viewer** — browse text files, syntax-highlighted source code, images (Kitty protocol), archives
- **Archive support** — browse ZIP, LHA/LZH, Amstrad DSK, Commodore D64 disk images as directories
- **Remote connections** — SFTP file browsing and IMAP email browsing
- **Lua plugin system** — extend KKC with archive and viewer plugins written in Lua 5.4
- **Themes** — 100+ colour themes (easily swappable from the menu)
- **File search** — recursive search with name and content filters
- **Quick search** — type-ahead filtering directly in the panel

---

## Installation

### Homebrew (macOS / Linux)

```sh
brew tap redbug26/kkc-rust https://github.com/redbug26/kkc-rust
brew install redbug26/kkc-rust/kkc
```

> **Note:** the two commands are both required — `brew tap` must be run first with the full URL since the repository is not named `homebrew-*`.

To always get the latest development build from source:

```sh
brew install --HEAD redbug26/kkc-rust/kkc
```

### Build from source

Prerequisites: [Rust](https://rustup.rs/) stable toolchain — **Linux only:** also `libssl-dev`.

```sh
git clone https://github.com/redbug26/kkc-rust.git
cd kkc-rust
cargo build --release
./target/release/kkc
```

---

## Keyboard shortcuts

### Navigation

| Key | Action |
|-----|--------|
| `↑` / `↓` | Move cursor |
| `PageUp` / `PageDown` | Scroll page |
| `Home` / `End` | First / last entry |
| `Tab` | Switch active panel |
| `Enter` | Open file or enter directory |
| `Backspace` | Go to parent directory |

### File operations

| Key | Action |
|-----|--------|
| `F1` | Help |
| `F2` | Menu |
| `F3` | View file (internal viewer) |
| `F4` | Edit file (external editor) |
| `F5` | Copy |
| `F6` | Move |
| `Shift+F6` | Rename |
| `F7` | Create directory |
| `F8` | Delete |
| `F10` / `q` | Quit |

### Selection

| Key | Action |
|-----|--------|
| `Insert` / `Space` | Toggle selection |
| `+` | Select by pattern |
| `-` | Deselect by pattern |
| `*` | Invert selection |

### Sorting (Ctrl+Fx)

| Key | Sort by |
|-----|---------|
| `Ctrl+F1` | Name |
| `Ctrl+F2` | Extension |
| `Ctrl+F3` | Date |
| `Ctrl+F4` | Size |
| `Ctrl+F5` | Unsorted |

### Other

| Key | Action |
|-----|--------|
| `Ctrl+H` | Show / hide hidden files |
| `Ctrl+R` | Reload panels |
| `Ctrl+D` | Directory bookmarks |
| `Ctrl+F` | Connect to remote (SFTP / IMAP) |
| `Ctrl+U` | Drop to terminal |
| `Alt+F4` | File identification view |
| `Alt+F7` | Search panel |

### Viewer shortcuts

| Key | Action |
|-----|--------|
| `↑` / `↓` / `PgUp` / `PgDn` | Scroll |
| `/` | Search in document |
| `n` | Next search match |
| `F4` | Change viewer mode / plugin |
| `Esc` | Close viewer |

---

## Bundled plugins

| Plugin | Description |
|--------|-------------|
| `text_syntax` | Syntax highlighting for Rust, C/C++, Lua, JS/TS |
| `csv_viewer` | Formatted CSV table viewer |
| `json_viewer` | Formatted JSON viewer |
| `markdown_viewer` | Rendered Markdown viewer |
| `html_viewer` | HTML viewer |
| `eml_viewer` | Email (.eml) viewer |
| `pdf_file` | PDF viewer |
| `lha_lzh` | LHA/LZH archive support |
| `amstrad_dsk` | Amstrad CPC disk image browser |
| `commodore_d64` | Commodore 64 disk image browser |

Plugins can be installed as directories or as `.kkplug` ZIP bundles.  
Press `Enter` on a `.kkplug` file to install it automatically.

---

## Plugin development

See [docs/plugins.md](docs/plugins.md) for the full Lua plugin API reference.

---

## Themes

KKC ships with 100+ themes. Switch theme from **F2 → Options → Theme**.  
Theme files are `.toml` files located in `assets/themes/`.

