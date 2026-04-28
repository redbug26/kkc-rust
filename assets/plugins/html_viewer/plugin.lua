local kkc = require("kkc")

-- ── span / line helpers ──────────────────────────────────────────────────

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

local function line(...)
    return { ... }
end

-- ── text utilities ───────────────────────────────────────────────────────

local function text_len(text)
    return utf8.len(text) or #text
end

local function truncate_text(text, width)
    if text_len(text) <= width then return text end
    local out = {}
    local count = 0
    for _, code in utf8.codes(text) do
        if count >= math.max(0, width - 1) then break end
        out[#out + 1] = utf8.char(code)
        count = count + 1
    end
    return table.concat(out) .. "…"
end

local function pad_right(text, width)
    text = truncate_text(text, width)
    return text .. string.rep(" ", math.max(0, width - text_len(text)))
end

local function is_html_path(path)
    local ext = path:match("%.([^%.\\/]+)$")
    if not ext then return false end
    ext = ext:lower()
    return ext == "html" or ext == "htm" or ext == "xhtml"
end

local function read_all(path)
    local file, err = io.open(path, "rb")
    if not file then return nil, err end
    local data = file:read("*a")
    file:close()
    return data
end

local function decode_entity(entity)
    entity = entity or ""
    local lower = entity:lower()
    local named = {
        nbsp = " ",
        lt = "<",
        gt = ">",
        amp = "&",
        quot = '"',
        apos = "'",
        copy = "©",
        reg = "®",
        trade = "™",
        euro = "€",
        mdash = "—",
        ndash = "–",
        hellip = "…",
        lsquo = "‘",
        rsquo = "’",
        ldquo = "“",
        rdquo = "”",
    }
    if named[lower] then return named[lower] end
    local dec = lower:match("^#(%d+)$")
    if dec and utf8 and utf8.char then
        local ok, value = pcall(utf8.char, tonumber(dec))
        if ok then return value end
    end
    local hex = lower:match("^#x(%x+)$")
    if hex and utf8 and utf8.char then
        local ok, value = pcall(utf8.char, tonumber(hex, 16))
        if ok then return value end
    end
    return "&" .. entity .. ";"
end

local function decode_entities(text)
    return (text:gsub("&([^;%s]+);", decode_entity))
end

local function strip_control(text)
    return (text:gsub("[%z\1-\8\11\12\14-\31\127]", " "))
end

local function attr_value(tag, attr)
    local lower = tag:lower()
    local start = lower:find(attr:lower() .. "%s*=")
    if not start then return nil end
    local rest = tag:sub(start):gsub("^[^=]+=%s*", "")
    local quote = rest:sub(1, 1)
    if quote == '"' or quote == "'" then
        local value = rest:sub(2):match("^(.-)" .. quote)
        return value and decode_entities(value) or nil
    end
    local value = rest:match("^([^%s>]+)")
    return value and decode_entities(value) or nil
end

-- ── link-href display heuristic ──────────────────────────────────────────
-- Only show the href when it adds value (short relative or meaningful absolute).
-- Never show CDN/framework/fragment/JS URLs.

local function should_show_href(href)
    if not href or href == "" then return false end
    if href:sub(1, 1) == "#" then return false end      -- page fragment
    if href:match("^javascript:") then return false end -- pseudo-protocol
    if href:match("cdn%.") then return false end        -- any CDN domain
    if href:match("jsdelivr") or href:match("bootstrap") then return false end
    if href:match("cloudflare") or href:match("googleapis") then return false end
    if href:match("unpkg%.com") then return false end
    if #href > 55 then return false end -- too long to be useful
    return true
end

-- Tags whose entire content (including children) should be silently skipped
local SKIP_TAG = { head = true, nav = true, noscript = true }

-- Void (self-closing) elements that never have children
local VOID_TAG = {
    area = true,
    base = true,
    br = true,
    col = true,
    embed = true,
    hr = true,
    img = true,
    input = true,
    link = true,
    meta = true,
    param = true,
    source = true,
    track = true,
    wbr = true,
}

local function doc_new(width)
    return {
        lines = {},
        current = {},
        col = 0,
        width = math.max(40, (tonumber(width) or 100) - 4),
    }
end

local function doc_flush(doc)
    if #doc.current > 0 then
        table.insert(doc.lines, doc.current)
        doc.current = {}
        doc.col = 0
    end
end

local function doc_blank(doc)
    doc_flush(doc)
    local last = doc.lines[#doc.lines]
    if last and #last == 0 then return end
    table.insert(doc.lines, {})
end

local function doc_text(doc, text, fg, bold)
    if not text or text == "" then return end
    table.insert(doc.current, span(text, fg, bold))
    doc.col = doc.col + text_len(text)
end

-- Emit a full-width horizontal rule on its own line
local function doc_rule(doc, char, fg, len)
    doc_flush(doc)
    local w = len or doc.width
    doc_text(doc, string.rep(char, w), fg or "darkgray")
    doc_flush(doc)
end

local function doc_wrap_word(doc, word, fg, bold, prefix)
    local len = text_len(word)
    if doc.col > 0 and doc.col + len > doc.width then
        doc_flush(doc)
        if prefix and prefix ~= "" then
            doc_text(doc, prefix, "darkgray")
        end
    end
    doc_text(doc, word, fg, bold)
end

local function append_text(doc, raw, ctx)
    raw = strip_control(decode_entities(raw or ""))
    if raw == "" then return end

    -- <pre> content: preserve whitespace and newlines
    if ctx.pre then
        for part, nl in raw:gmatch("([^\n]*)(\n?)") do
            if part ~= "" then doc_text(doc, part, "lightgreen") end
            if nl ~= "" then doc_flush(doc) end
        end
        return
    end

    -- Collapse whitespace for normal flow; trim leading space on new lines
    raw = raw:gsub("%s+", " ")
    if doc.col == 0 then raw = raw:gsub("^%s+", "") end
    if raw == "" then return end

    -- Colour priority: kbd > code > heading > link > strong > em > small > quote > default
    local fg, bold
    if ctx.kbd then
        fg = "lightgreen"; bold = true
    elseif ctx.code then
        fg = "lightyellow"; bold = false
    elseif ctx.heading then
        local lvl = ctx.heading_level or 1
        if lvl == 1 then
            fg = "lightyellow"; bold = true
        elseif lvl == 2 then
            fg = "lightcyan"; bold = true
        elseif lvl == 3 then
            fg = "cyan"; bold = true
        elseif lvl == 4 then
            fg = "white"; bold = true
        else
            fg = "gray"; bold = true
        end
    elseif ctx.link then
        fg = "lightcyan"; bold = false
    elseif ctx.strong then
        fg = "white"; bold = true
    elseif ctx.em then
        fg = "lightmagenta"; bold = false
    elseif ctx.small then
        fg = "gray"; bold = false
    elseif ctx.quote and ctx.quote > 0 then
        fg = "gray"; bold = false
    else
        fg = "white"; bold = false
    end

    local prefix = (ctx.quote and ctx.quote > 0)
        and string.rep("│ ", ctx.quote)
        or ""

    for token in raw:gmatch("%S+%s*") do
        local word  = token:gsub("%s+$", "")
        local space = token:match("%s+$") and " " or ""
        if word ~= "" then
            if doc.col == 0 and prefix ~= "" then doc_text(doc, prefix, "darkgray") end
            doc_wrap_word(doc, word, fg, bold, prefix)
            if space ~= "" and doc.col < doc.width then doc_text(doc, " ", fg, bold) end
        end
    end
end

local function tag_name(tag)
    return (tag:lower():match("^%s*/?%s*([%w:_-]+)") or "")
end

local function plain_html_text(input)
    input = input:gsub("<[bB][rR][^>]*>", " ")
    input = input:gsub("<[^>]+>", "")
    input = decode_entities(input)
    input = strip_control(input)
    return input:gsub("%s+", " "):match("^%s*(.-)%s*$")
end

local function table_rows(markup)
    local rows = {}
    for row in markup:gmatch("<%s*[tT][rR][^>]*>(.-)<%s*/%s*[tT][rR]%s*>") do
        local cells = {}
        local header = false
        for tag, cell in row:gmatch("<%s*([tT][hHdD])[^>]*>(.-)<%s*/%s*%1%s*>") do
            local lower = tag:lower()
            if lower == "th" then header = true end
            cells[#cells + 1] = plain_html_text(cell)
        end
        if #cells > 0 then
            rows[#rows + 1] = { cells = cells, header = header }
        end
    end
    return rows
end

local function render_table(markup, width)
    local rows = table_rows(markup)
    if #rows == 0 then return {} end

    local col_count = 0
    for _, row in ipairs(rows) do col_count = math.max(col_count, #row.cells) end
    local widths = {}
    for col = 1, col_count do widths[col] = 3 end
    for _, row in ipairs(rows) do
        for col = 1, col_count do
            widths[col] = math.max(widths[col], text_len(row.cells[col] or ""))
        end
    end

    local max_width = math.max(30, tonumber(width) or 100)
    local sep_width = math.max(0, col_count - 1) * 3
    local function total_width()
        local total = sep_width
        for _, w in ipairs(widths) do total = total + w end
        return total
    end
    while total_width() > max_width do
        local widest, widest_idx = 0, 1
        for idx, w in ipairs(widths) do
            if w > widest then widest, widest_idx = w, idx end
        end
        if widest <= 8 then break end
        widths[widest_idx] = widest - 1
    end

    local out = {}

    -- Box-drawing separator row: top/header-underline/bottom variants
    -- top:    ┌──┬──┐   header-sep: ├──┼──┤   bottom: └──┴──┘
    local function separator(lc, mc, rc, hc)
        local spans = { span(lc, "darkgray") }
        for c = 1, col_count do
            if c > 1 then spans[#spans + 1] = span(mc, "darkgray") end
            spans[#spans + 1] = span(string.rep(hc, widths[c] + 2), "darkgray")
        end
        spans[#spans + 1] = span(rc, "darkgray")
        out[#out + 1] = spans
    end

    separator("┌", "┬", "┐", "─") -- top border

    for ri, row in ipairs(rows) do
        -- data row: │ cell │ cell │
        local spans = { span("│", "darkgray") }
        for c = 1, col_count do
            local cell_fg = row.header and "lightcyan" or "white"
            spans[#spans + 1] = span(" ", "darkgray")
            spans[#spans + 1] = span(
                pad_right(row.cells[c] or "", widths[c]),
                cell_fg, row.header
            )
            spans[#spans + 1] = span(" │", "darkgray")
        end
        out[#out + 1] = spans
        if row.header and ri == 1 then
            separator("├", "┼", "┤", "─") -- header underline
        end
    end

    separator("└", "┴", "┘", "─") -- bottom border
    return out
end

local function parse_html(input, width)
    input = input:gsub("\r\n", "\n"):gsub("\r", "\n")
    -- Strip HTML comments, then script/style blocks
    input = input:gsub("<!%-%-.-%-%->", "")
    input = input:gsub("<[sS][cC][rR][iI][pP][tT][^>]*>.-</[sS][cC][rR][iI][pP][tT]%s*>", "")
    input = input:gsub("<[sS][tT][yY][lL][eE][^>]*>.-</[sS][tT][yY][lL][eE]%s*>", "")

    -- Extract <title> for use in the header banner
    local title_text = input:match("<[tT][iI][tT][lL][eE][^>]*>%s*(.-)%s*</[tT][iI][tT][lL][eE]>")
    if title_text then
        title_text = decode_entities(title_text):gsub("%s+", " ")
    end

    local doc        = doc_new(width)
    local ctx        = {
        strong = false,
        em = false,
        code = false,
        kbd = false,
        pre = false,
        small = false,
        quote = 0,
        heading = false,
        heading_level = 0,
        link = nil,
        skip = 0,   -- depth counter: >0 means we are inside a SKIP_TAG block
    }
    local lists      = {}
    local pos        = 1

    -- Nested ul bullet symbols by depth
    local UL_BULLETS = { "• ", "◦ ", "▸ " }

    while pos <= #input do
        local tag_start = input:find("<", pos, true)
        if not tag_start then
            if ctx.skip == 0 then append_text(doc, input:sub(pos), ctx) end
            break
        end
        if tag_start > pos and ctx.skip == 0 then
            append_text(doc, input:sub(pos, tag_start - 1), ctx)
        end

        local tag_end = input:find(">", tag_start + 1, true)
        if not tag_end then
            if ctx.skip == 0 then append_text(doc, input:sub(tag_start), ctx) end
            break
        end

        local tag      = input:sub(tag_start + 1, tag_end - 1)
        local closing  = tag:match("^%s*/") ~= nil
        local name     = tag_name(tag)
        local self_cls = tag:match("/%s*$") ~= nil or VOID_TAG[name] or false

        -- ── skip-depth tracking (inside head/nav/noscript) ───────────────
        if ctx.skip > 0 then
            if not closing and not self_cls then
                ctx.skip = ctx.skip + 1
            elseif closing then
                ctx.skip = math.max(0, ctx.skip - 1)
            end
            pos = tag_end + 1
            goto continue
        end

        -- ── opening tags ─────────────────────────────────────────────────
        if not closing then
            -- Enter skip zone (head / nav / noscript)?
            if SKIP_TAG[name] then
                if not self_cls then ctx.skip = 1 end
                pos = tag_end + 1
                goto continue
            end

            -- Absorb entire <table>…</table> block at once
            if name == "table" then
                local _, tbl_end = input:lower():find("</%s*table%s*>", tag_end + 1)
                if tbl_end then
                    doc_blank(doc)
                    for _, tl in ipairs(render_table(input:sub(tag_start, tbl_end), doc.width)) do
                        table.insert(doc.lines, tl)
                    end
                    doc_blank(doc)
                    pos = tbl_end + 1
                    goto continue
                end

                -- ── headings ─────────────────────────────────────────────────
            elseif name:match("^h[1-6]$") then
                local lvl = tonumber(name:sub(2)) or 1
                doc_blank(doc)
                if lvl == 1 then
                    doc_rule(doc, "═", "darkgray")
                    doc_blank(doc)
                elseif lvl == 2 then
                    doc_blank(doc)
                end
                -- Leading prefix symbol by level
                local pfx = ({ nil, "◆ ", "▸ ", "• " })[math.min(lvl, 4)]
                if pfx then doc_text(doc, pfx, "darkgray") end
                ctx.heading       = true
                ctx.heading_level = lvl

                -- ── block-level elements ──────────────────────────────────────
            elseif name == "p" then
                if doc.col > 0 then doc_blank(doc) else doc_flush(doc) end
            elseif name == "div" or name == "section" or name == "article"
                or name == "header" or name == "footer" or name == "main"
                or name == "aside" or name == "figure" then
                doc_flush(doc)
            elseif name == "tr" then
                doc_flush(doc)

                -- ── line break / rule ─────────────────────────────────────────
            elseif name == "br" then
                doc_flush(doc)
            elseif name == "hr" then
                doc_blank(doc)
                doc_rule(doc, "─", "darkgray", math.min(doc.width, 72))
                doc_blank(doc)

                -- ── inline formatting ─────────────────────────────────────────
            elseif name == "strong" or name == "b" then
                ctx.strong = true
            elseif name == "em" or name == "i" or name == "cite" then
                ctx.em = true
            elseif name == "small" or name == "sub" or name == "sup" then
                ctx.small = true
            elseif name == "kbd" then
                ctx.kbd = true
                doc_text(doc, "[", "lightgreen", true)
            elseif name == "code" or name == "samp" or name == "var" then
                if not ctx.pre then doc_text(doc, "`", "darkgray") end
                ctx.code = true
            elseif name == "pre" then
                doc_blank(doc)
                ctx.pre = true

                -- ── blockquote ───────────────────────────────────────────────
            elseif name == "blockquote" then
                doc_blank(doc)
                ctx.quote = (ctx.quote or 0) + 1

                -- ── lists ────────────────────────────────────────────────────
            elseif name == "ul" then
                table.insert(lists, { kind = "ul", n = 0 })
                doc_flush(doc)
            elseif name == "ol" then
                table.insert(lists, { kind = "ol", n = 0 })
                doc_flush(doc)
            elseif name == "li" then
                doc_flush(doc)
                local list   = lists[#lists] or { kind = "ul", n = 0 }
                list.n       = list.n + 1
                local depth  = #lists
                local indent = string.rep("  ", math.max(0, depth - 1))
                local bullet
                if list.kind == "ol" then
                    bullet = tostring(list.n) .. ". "
                else
                    local bi = ((depth - 1) % #UL_BULLETS) + 1
                    bullet   = UL_BULLETS[bi]
                end
                doc_text(doc, indent .. bullet, "lightcyan", true)

                -- ── links ─────────────────────────────────────────────────────
            elseif name == "a" then
                ctx.link = attr_value(tag, "href") or true

                -- ── images ───────────────────────────────────────────────────
            elseif name == "img" then
                local alt = attr_value(tag, "alt")
                if alt and alt ~= "" then
                    append_text(doc, "[img: " .. alt .. "] ",
                        { small = true, quote = ctx.quote })
                end

                -- ── figcaption ───────────────────────────────────────────────
            elseif name == "figcaption" then
                doc_flush(doc)
                doc_text(doc, "▲ ", "darkgray")

                -- ── summary / details ─────────────────────────────────────────
            elseif name == "summary" then
                doc_flush(doc)
                doc_text(doc, "▼ ", "darkgray")

                -- ── button (keep text, wrap in brackets) ──────────────────────
            elseif name == "button" then
                doc_text(doc, "[", "darkgray")
            end

            -- ── closing tags ─────────────────────────────────────────────────
        else
            if name:match("^h[1-6]$") then
                local lvl         = tonumber(name:sub(2)) or 1
                ctx.heading       = false
                ctx.heading_level = 0
                doc_flush(doc)
                if lvl == 1 then
                    doc_blank(doc)
                    doc_rule(doc, "═", "darkgray")
                elseif lvl == 2 then
                    doc_rule(doc, "─", "darkgray", math.min(doc.width, 48))
                end
                doc_blank(doc)
            elseif name == "p" then
                doc_flush(doc)
            elseif name == "div" or name == "section" or name == "article"
                or name == "header" or name == "footer" or name == "main"
                or name == "aside" or name == "figure" then
                doc_flush(doc)
            elseif name == "tr" or name == "li" then
                doc_flush(doc)
            elseif name == "strong" or name == "b" then
                ctx.strong = false
            elseif name == "em" or name == "i" or name == "cite" then
                ctx.em = false
            elseif name == "small" or name == "sub" or name == "sup" then
                ctx.small = false
            elseif name == "kbd" then
                doc_text(doc, "]", "lightgreen", true)
                ctx.kbd = false
            elseif name == "code" or name == "samp" or name == "var" then
                ctx.code = false
                if not ctx.pre then doc_text(doc, "`", "darkgray") end
            elseif name == "pre" then
                ctx.pre = false
                doc_blank(doc)
            elseif name == "blockquote" then
                doc_blank(doc)
                ctx.quote = math.max(0, (ctx.quote or 0) - 1)
            elseif name == "ul" or name == "ol" then
                table.remove(lists)
                doc_flush(doc)
            elseif name == "a" then
                if type(ctx.link) == "string" and should_show_href(ctx.link) then
                    append_text(doc, " → " .. ctx.link,
                        { small = true, quote = ctx.quote })
                end
                ctx.link = nil
            elseif name == "button" then
                doc_text(doc, "]", "darkgray")
            end
        end

        pos = tag_end + 1
        ::continue::
    end

    doc_flush(doc)

    -- Collapse consecutive blank lines
    do
        local i = 2
        while i <= #doc.lines do
            if #doc.lines[i] == 0 and #doc.lines[i - 1] == 0 then
                table.remove(doc.lines, i)
            else
                i = i + 1
            end
        end
    end

    -- Remove trailing blank lines
    while #doc.lines > 1 and #doc.lines[#doc.lines] == 0 do
        table.remove(doc.lines)
    end

    if #doc.lines == 0 then
        return { line(span("(empty document)", "darkgray")) }, title_text
    end
    return doc.lines, title_text
end

local function render_html(path, mode, state, width)
    if mode ~= "text" or not is_html_path(path) then return nil end
    local input, err = read_all(path)
    if not input then
        return { line(span("Error: " .. tostring(err), "red", true)) }
    end

    local parsed, title = parse_html(input, width)
    local filename      = path:match("([^\\/]+)$") or path
    local display_title = (title and title ~= "") and title or filename

    local out           = {
        line(
            span("┌── ", "darkgray"),
            span(display_title, "lightyellow", true),
            span("  [" .. filename .. "]", "gray"),
            span("  " .. tostring(#input) .. " B", "darkgray")
        ),
        line(span("")),
    }
    for _, l in ipairs(parsed) do table.insert(out, l) end
    return out
end

kkc.register_viewer_plugin({
    name        = "html_viewer",
    version     = "2.0.0",
    description = "Rendered HTML viewer — heading hierarchy, box tables, kbd, smart links",
    modes       = { "text" },
    mime_types  = { "text/html", "application/xhtml+xml" },
    render      = render_html,
})
