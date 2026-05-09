local kkc = require("kkc")

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

local function line(...)
    return { ... }
end

local push

local function text_len(text)
    return utf8.len(text) or #text
end

local truncate_text

local function clone_span(source, text)
    return {
        text = text,
        fg = source.fg or "white",
        bg = source.bg or "black",
        bold = source.bold or false,
    }
end

local function text_chars(text)
    local chars = {}
    for _, code in utf8.codes(text) do chars[#chars + 1] = utf8.char(code) end
    if #chars == 0 and #text > 0 then
        for i = 1, #text do chars[#chars + 1] = text:sub(i, i) end
    end
    return chars
end

local function take_text(text, width)
    local out = {}
    local count = 0
    for _, ch in ipairs(text_chars(text)) do
        if count >= width then break end
        out[#out + 1] = ch
        count = count + 1
    end
    return table.concat(out)
end

local function split_wrap_tokens(text)
    local tokens = {}
    local current = {}
    local current_space = nil
    for _, ch in ipairs(text_chars(text)) do
        local is_space = ch:match("%s") ~= nil
        if current_space == nil or is_space == current_space then
            current[#current + 1] = ch
        else
            tokens[#tokens + 1] = { text = table.concat(current), space = current_space }
            current = { ch }
        end
        current_space = is_space
    end
    if #current > 0 then tokens[#tokens + 1] = { text = table.concat(current), space = current_space } end
    return tokens
end

local function append_span_text(line_spans, source_span, text)
    if text == "" then return end
    local last = line_spans[#line_spans]
    if last
        and last.fg == (source_span.fg or "white")
        and last.bg == (source_span.bg or "black")
        and last.bold == (source_span.bold or false) then
        last.text = last.text .. text
    else
        line_spans[#line_spans + 1] = clone_span(source_span, text)
    end
end

local function push_wrapped(out, prefix, spans, width)
    width = math.max(1, tonumber(width) or 80)
    prefix = prefix or ""
    local prefix_width = text_len(prefix)
    local continuation = string.rep(" ", prefix_width)
    local content_width = math.max(1, width - prefix_width)
    local prefix_span = span(prefix, "darkgray", false)
    local continuation_span = span(continuation, "darkgray", false)
    local line_spans = prefix ~= "" and { prefix_span } or {}
    local line_width = 0
    local line_started = false

    local function trim_trailing_spaces()
        while #line_spans > 0 do
            local last = line_spans[#line_spans]
            local trimmed = (last.text or ""):gsub("%s+$", "")
            if trimmed == last.text then break end
            line_width = math.max(0, line_width - (text_len(last.text) - text_len(trimmed)))
            if trimmed == "" then
                table.remove(line_spans)
            else
                last.text = trimmed
                break
            end
        end
    end

    local function flush()
        trim_trailing_spaces()
        push(out, line_spans)
        line_spans = continuation ~= "" and { continuation_span } or {}
        line_width = 0
        line_started = false
    end

    local function append_token(source_span, token)
        local token_text = token.text
        local token_width = text_len(token_text)
        if token.space and not line_started then return end
        if line_started and line_width + token_width > content_width then
            flush()
            if token.space then return end
        end
        while token_width > content_width do
            local part = take_text(token_text, math.max(1, content_width - line_width))
            if part == "" then
                flush()
                part = take_text(token_text, content_width)
            end
            append_span_text(line_spans, source_span, part)
            token_text = token_text:sub(#part + 1)
            flush()
            token_width = text_len(token_text)
        end
        if token_text ~= "" then
            append_span_text(line_spans, source_span, token_text)
            line_width = line_width + token_width
            if not token.space then line_started = true end
        end
    end

    for _, source_span in ipairs(spans) do
        for _, token in ipairs(split_wrap_tokens(source_span.text or "")) do
            append_token(source_span, token)
        end
    end
    if line_started or line_width > 0 then flush() end
end

function truncate_text(text, width)
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

local function is_markdown_path(path)
    local ext = path:match("%.([^%.\\/]+)$")
    if not ext then return false end
    ext = ext:lower()
    return ext == "md" or ext == "markdown" or ext == "mdown" or ext == "mkd"
end

local function read_all(path)
    local file, err = io.open(path, "rb")
    if not file then return nil, err end
    local data = file:read("*a")
    file:close()
    return data
end

function push(out, spans)
    table.insert(out, spans)
end

local function parse_inline(text, base_fg, base_bold)
    local spans = {}
    local i = 1
    local function add(t, fg, bold)
        if t ~= "" then table.insert(spans, span(t, fg or base_fg or "white", bold or base_bold or false)) end
    end

    while i <= #text do
        local code_start, code_end, code_text = text:find("`([^`]+)`", i)
        local link_start, link_end, link_text, link_url = text:find("%[([^%]]+)%]%(([^%)]+)%)", i)
        local bold_start, bold_end, bold_text = text:find("%*%*([^*]+)%*%*", i)
        local em_start, em_end, em_text = text:find("%*([^*]+)%*", i)

        local best_start, best_end, kind, a, b = nil, nil, nil, nil, nil
        for _, item in ipairs({
            { code_start, code_end, "code", code_text },
            { link_start, link_end, "link", link_text, link_url },
            { bold_start, bold_end, "bold", bold_text },
            { em_start,   em_end,   "em",   em_text },
        }) do
            if item[1] and (not best_start or item[1] < best_start) then
                best_start, best_end, kind, a, b = item[1], item[2], item[3], item[4], item[5]
            end
        end

        if not best_start then
            add(text:sub(i))
            break
        end
        if best_start > i then add(text:sub(i, best_start - 1)) end
        if kind == "code" then
            add(a, "lightyellow", false)
        elseif kind == "link" then
            add(a, "lightcyan", true)
            add(" (" .. b .. ")", "darkgray", false)
        elseif kind == "bold" then
            add(a, base_fg or "white", true)
        elseif kind == "em" then
            add(a, "lightmagenta", base_bold)
        end
        i = best_end + 1
    end
    return spans
end

local function append_prefixed(out, prefix, text, fg, bold, width)
    push_wrapped(out, prefix, parse_inline(text, fg, bold), width)
end

local function split_table_row(row)
    row = row:gsub("^%s*|", ""):gsub("|%s*$", "")
    local cells = {}
    for cell in row:gmatch("([^|]+)") do
        cells[#cells + 1] = cell:match("^%s*(.-)%s*$")
    end
    return cells
end

local function is_table_separator(line_text)
    if not line_text:find("|", 1, true) then return false end
    local count = 0
    for _, cell in ipairs(split_table_row(line_text)) do
        if not cell:match("^:?-+:?$") then return false end
        count = count + 1
    end
    return count > 0
end

local function table_alignment(separator)
    local align = {}
    for _, cell in ipairs(split_table_row(separator)) do
        local left = cell:match("^%s*:")
        local right = cell:match(":%s*$")
        align[#align + 1] = left and right and "center" or right and "right" or "left"
    end
    return align
end

local function pad_cell(text, width, align)
    text = truncate_text(text, width)
    local gap = math.max(0, width - text_len(text))
    if align == "right" then
        return string.rep(" ", gap) .. text
    elseif align == "center" then
        local left = math.floor(gap / 2)
        return string.rep(" ", left) .. text .. string.rep(" ", gap - left)
    end
    return text .. string.rep(" ", gap)
end

local function render_table(out, rows, separator, max_width)
    if #rows == 0 then return end
    local align = table_alignment(separator)
    local parsed = {}
    local col_count = 0
    for _, row in ipairs(rows) do
        local cells = split_table_row(row)
        parsed[#parsed + 1] = cells
        col_count = math.max(col_count, #cells)
    end
    local widths = {}
    for col = 1, col_count do widths[col] = 3 end
    for _, cells in ipairs(parsed) do
        for col = 1, col_count do
            widths[col] = math.max(widths[col], text_len(cells[col] or ""))
        end
    end

    max_width = math.max(10, tonumber(max_width) or 100)
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

    local function push_separator()
        local spans = {}
        for col = 1, col_count do
            if col > 1 then spans[#spans + 1] = span("─┼─", "darkgray") end
            spans[#spans + 1] = span(string.rep("─", widths[col]), "darkgray")
        end
        push(out, spans)
    end

    for row_idx, cells in ipairs(parsed) do
        local spans = {}
        for col = 1, col_count do
            if col > 1 then spans[#spans + 1] = span(" │ ", "darkgray") end
            local text = pad_cell(cells[col] or "", widths[col], align[col])
            spans[#spans + 1] = span(text, row_idx == 1 and "lightcyan" or "white", row_idx == 1)
        end
        push(out, spans)
        if row_idx == 1 then push_separator() end
    end
end

local function render_markdown(path, mode, state, width)
    if mode ~= "text" or not is_markdown_path(path) then return nil end
    local input, err = read_all(path)
    if not input then
        return { line(span("Error opening Markdown file: " .. tostring(err), "red", true)) }
    end

    input = input:gsub("\r\n", "\n"):gsub("\r", "\n")
    local out = {}
    push_wrapped(out, "", {
        span("Markdown", "yellow", true),
        span("  file: ", "gray"),
        span(path:match("([^\\/]+)$") or path, "white"),
        span("  bytes: ", "gray"),
        span(tostring(#input), "lightmagenta"),
    }, width)
    push(out, line(span("")))

    local in_code = false
    local code_lang = ""
    local source_lines = {}
    for raw in (input .. "\n"):gmatch("(.-)\n") do source_lines[#source_lines + 1] = raw end
    local idx = 1
    while idx <= #source_lines do
        local raw = source_lines[idx]
        local line_text = raw:gsub("\t", "    ")

        local fence = line_text:match("^%s*```%s*(.*)$") or line_text:match("^%s*~~~%s*(.*)$")
        if fence then
            in_code = not in_code
            code_lang = in_code and fence or ""
            push_wrapped(out, "", { span(in_code and ("┌─ code " .. code_lang) or "└─", "darkgray") }, width)
        elseif in_code then
            push_wrapped(out, "  ", { span(line_text, "lightgreen") }, width)
        elseif line_text:match("^%s*$") then
            push(out, line(span("")))
        else
            local h, title = line_text:match("^(#+)%s+(.+)$")
            if h then
                push_wrapped(out, string.rep("#", #h) .. " ", { span(title, "yellow", true) }, width)
            elseif line_text:match("^%s*>") then
                local text = line_text:gsub("^%s*>%s?", "")
                append_prefixed(out, "│ ", text, "gray", false, width)
            elseif line_text:match("^%s*[-*+]%s+") then
                local indent, text = line_text:match("^(%s*)[-*+]%s+(.+)$")
                append_prefixed(out, string.rep(" ", math.floor(#indent / 2) * 2) .. "• ", text, "white", false, width)
            elseif line_text:match("^%s*%d+[.)]%s+") then
                local indent, num, text = line_text:match("^(%s*)(%d+)[.)]%s+(.+)$")
                append_prefixed(out, string.rep(" ", math.floor(#indent / 2) * 2) .. num .. ". ", text, "white", false,
                    width)
            elseif line_text:find("|", 1, true)
                and source_lines[idx + 1]
                and is_table_separator(source_lines[idx + 1]) then
                local rows = { line_text }
                local separator = source_lines[idx + 1]
                idx = idx + 2
                while idx <= #source_lines and source_lines[idx]:find("|", 1, true) and not source_lines[idx]:match("^%s*$") do
                    rows[#rows + 1] = source_lines[idx]
                    idx = idx + 1
                end
                render_table(out, rows, separator, width)
                goto continue
            elseif line_text:match("^%s*[-*_][%s%-*_]+$") then
                push(out, line(span(string.rep("─", math.min(tonumber(width) or 80, 100)), "darkgray")))
            else
                push_wrapped(out, "", parse_inline(line_text, "white", false), width)
            end
        end
        idx = idx + 1
        ::continue::
    end

    return out
end

kkc.register_viewer_plugin({
    modes = { "text" },
    render = render_markdown,
})
