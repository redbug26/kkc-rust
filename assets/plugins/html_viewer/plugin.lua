local kkc = require("kkc")

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

local function line(...)
    return { ... }
end

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
        nbsp = " ", lt = "<", gt = ">", amp = "&", quot = '"', apos = "'",
        copy = "©", reg = "®", trade = "™", euro = "€", mdash = "—", ndash = "–",
        hellip = "…", lsquo = "‘", rsquo = "’", ldquo = "“", rdquo = "”",
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

local function doc_new(width)
    return {
        lines = {},
        current = {},
        col = 0,
        width = math.max(30, (tonumber(width) or 100) - 2),
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
    if text == "" then return end
    table.insert(doc.current, span(text, fg, bold))
    doc.col = doc.col + text_len(text)
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

    local fg = ctx.code and "lightyellow"
        or ctx.link and "lightcyan"
        or ctx.heading and "yellow"
        or ctx.quote and "gray"
        or "white"
    local bold = ctx.bold or ctx.heading or ctx.link or false
    local prefix = ctx.quote and string.rep("│ ", ctx.quote) or ""

    if ctx.pre then
        for part, nl in raw:gmatch("([^\n]*)(\n?)") do
            if part ~= "" then doc_text(doc, part, "lightgreen") end
            if nl ~= "" then doc_flush(doc) end
        end
        return
    end

    raw = raw:gsub("%s+", " ")
    for token in raw:gmatch("%S+%s*") do
        local word = token:gsub("%s+$", "")
        local space = token:match("%s+$") and " " or ""
        if word ~= "" then
            if doc.col == 0 and prefix ~= "" then doc_text(doc, prefix, "darkgray") end
            doc_wrap_word(doc, word, fg, bold, prefix)
            if space ~= "" and doc.col < doc.width then doc_text(doc, " ", fg, bold) end
        end
    end
end

local function start_block(doc, ctx, blank_before)
    if blank_before then doc_blank(doc) else doc_flush(doc) end
    if ctx.quote and ctx.quote > 0 then
        doc_text(doc, string.rep("│ ", ctx.quote), "darkgray")
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
    local function separator()
        local spans = {}
        for col = 1, col_count do
            if col > 1 then spans[#spans + 1] = span("─┼─", "darkgray") end
            spans[#spans + 1] = span(string.rep("─", widths[col]), "darkgray")
        end
        out[#out + 1] = spans
    end

    for row_idx, row in ipairs(rows) do
        local spans = {}
        for col = 1, col_count do
            if col > 1 then spans[#spans + 1] = span(" │ ", "darkgray") end
            spans[#spans + 1] = span(pad_right(row.cells[col] or "", widths[col]), row.header and "lightcyan" or "white", row.header)
        end
        out[#out + 1] = spans
        if row.header and row_idx == 1 then separator() end
    end
    return out
end

local function parse_html(input, width)
    input = input:gsub("\r\n", "\n"):gsub("\r", "\n")
    input = input:gsub("<!%-%-.-%-%->", "")
    input = input:gsub("<[sS][cC][rR][iI][pP][tT][^>]*>.-</[sS][cC][rR][iI][pP][tT]>", "")
    input = input:gsub("<[sS][tT][yY][lL][eE][^>]*>.-</[sS][tT][yY][lL][eE]>", "")

    local doc = doc_new(width)
    local ctx = { bold = false, code = false, pre = false, quote = 0, heading = false, link = nil }
    local lists = {}
    local pos = 1

    while pos <= #input do
        local tag_start = input:find("<", pos, true)
        if not tag_start then
            append_text(doc, input:sub(pos), ctx)
            break
        end
        if tag_start > pos then
            append_text(doc, input:sub(pos, tag_start - 1), ctx)
        end

        local tag_end = input:find(">", tag_start + 1, true)
        if not tag_end then
            append_text(doc, input:sub(tag_start), ctx)
            break
        end

        local tag = input:sub(tag_start + 1, tag_end - 1)
        local lower = tag:lower():gsub("^%s+", "")
        local closing = lower:sub(1, 1) == "/"
        local name = tag_name(tag)

        if not closing then
            if name == "table" then
                local close_start, close_end = input:lower():find("</%s*table%s*>", tag_end + 1)
                if close_start then
                    doc_blank(doc)
                    local table_markup = input:sub(tag_start, close_end)
                    for _, table_line in ipairs(render_table(table_markup, doc.width)) do
                        table.insert(doc.lines, table_line)
                    end
                    doc_blank(doc)
                    pos = close_end + 1
                    goto continue
                end
            elseif name == "h1" or name == "h2" or name == "h3" or name == "h4" or name == "h5" or name == "h6" then
                start_block(doc, ctx, true)
                ctx.heading = true
                local level = tonumber(name:sub(2)) or 1
                doc_text(doc, string.rep("#", level) .. " ", "darkgray", true)
            elseif name == "p" or name == "div" or name == "section" or name == "article"
                or name == "header" or name == "footer" or name == "main" or name == "tr" then
                start_block(doc, ctx, name == "p")
            elseif name == "br" then
                doc_flush(doc)
            elseif name == "hr" then
                doc_blank(doc)
                doc_text(doc, string.rep("─", math.min(doc.width, 72)), "darkgray")
                doc_blank(doc)
            elseif name == "strong" or name == "b" or name == "th" then
                ctx.bold = true
            elseif name == "em" or name == "i" then
                ctx.bold = true
            elseif name == "code" then
                ctx.code = true
            elseif name == "pre" then
                doc_blank(doc)
                ctx.pre = true
            elseif name == "blockquote" then
                doc_blank(doc)
                ctx.quote = (ctx.quote or 0) + 1
            elseif name == "ul" then
                table.insert(lists, { kind = "ul", n = 0 })
                doc_flush(doc)
            elseif name == "ol" then
                table.insert(lists, { kind = "ol", n = 0 })
                doc_flush(doc)
            elseif name == "li" then
                doc_flush(doc)
                local list = lists[#lists] or { kind = "ul", n = 0 }
                list.n = list.n + 1
                local indent = string.rep("  ", math.max(0, #lists - 1))
                local bullet = list.kind == "ol" and (tostring(list.n) .. ". ") or "• "
                doc_text(doc, indent .. bullet, "lightcyan", true)
            elseif name == "a" then
                ctx.link = attr_value(tag, "href") or true
            elseif name == "img" then
                local alt = attr_value(tag, "alt") or attr_value(tag, "src") or "image"
                append_text(doc, "[image: " .. alt .. "]", { code = false, link = true, quote = ctx.quote })
            elseif name == "td" or name == "th" then
                if doc.col > 0 then doc_text(doc, "  │  ", "darkgray") end
                if name == "th" then ctx.bold = true end
            end
        else
            if name:match("^h%d$") then
                ctx.heading = false
                doc_blank(doc)
            elseif name == "p" or name == "div" or name == "section" or name == "article"
                or name == "tr" or name == "li" then
                doc_flush(doc)
            elseif name == "strong" or name == "b" or name == "th" or name == "em" or name == "i" then
                ctx.bold = false
            elseif name == "code" then
                ctx.code = false
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
                if type(ctx.link) == "string" then
                    append_text(doc, " (" .. ctx.link .. ")", { code = false, link = false, quote = ctx.quote })
                end
                ctx.link = nil
            end
        end

        pos = tag_end + 1
        ::continue::
    end

    doc_flush(doc)
    while #doc.lines > 1 and #doc.lines[#doc.lines] == 0 do table.remove(doc.lines) end
    if #doc.lines == 0 then return { line(span("(empty document)", "darkgray")) } end
    return doc.lines
end

local function render_html(path, mode, state, width)
    if mode ~= "text" or not is_html_path(path) then return nil end
    local input, err = read_all(path)
    if not input then
        return { line(span("Error opening HTML file: " .. tostring(err), "red", true)) }
    end

    local out = {
        line(span("HTML", "yellow", true), span("  file: ", "gray"), span(path:match("([^\\/]+)$") or path, "white"), span("  bytes: ", "gray"), span(tostring(#input), "lightmagenta")),
        line(span("")),
    }
    for _, parsed in ipairs(parse_html(input, width)) do table.insert(out, parsed) end
    return out
end

kkc.register_viewer_plugin({
    name = "html_viewer",
    version = "1.0.0",
    description = "Rendered HTML text viewer",
    modes = { "text" },
    extensions = { "html", "htm", "xhtml" },
    render = render_html,
})
