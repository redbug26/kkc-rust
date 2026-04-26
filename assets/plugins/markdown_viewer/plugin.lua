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

local function push(out, spans)
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
            { em_start, em_end, "em", em_text },
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

local function append_prefixed(out, prefix, text, fg, bold)
    local spans = { span(prefix, "darkgray", false) }
    for _, s in ipairs(parse_inline(text, fg, bold)) do table.insert(spans, s) end
    push(out, spans)
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

    max_width = math.max(30, tonumber(max_width) or 100)
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
    local out = {
        line(span("Markdown", "yellow", true), span("  file: ", "gray"), span(path:match("([^\\/]+)$") or path, "white"), span("  bytes: ", "gray"), span(tostring(#input), "lightmagenta")),
        line(span("")),
    }

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
            push(out, line(span(in_code and ("┌─ code " .. code_lang) or "└─", "darkgray")))
        elseif in_code then
            push(out, line(span("  " .. line_text, "lightgreen")))
        elseif line_text:match("^%s*$") then
            push(out, line(span("")))
        else
            local h, title = line_text:match("^(#+)%s+(.+)$")
            if h then
                push(out, line(span(string.rep("#", #h) .. " ", "darkgray", true), span(title, "yellow", true)))
            elseif line_text:match("^%s*>") then
                local text = line_text:gsub("^%s*>%s?", "")
                append_prefixed(out, "│ ", text, "gray", false)
            elseif line_text:match("^%s*[-*+]%s+") then
                local indent, text = line_text:match("^(%s*)[-*+]%s+(.+)$")
                append_prefixed(out, string.rep(" ", math.floor(#indent / 2) * 2) .. "• ", text, "white", false)
            elseif line_text:match("^%s*%d+[.)]%s+") then
                local indent, num, text = line_text:match("^(%s*)(%d+)[.)]%s+(.+)$")
                append_prefixed(out, string.rep(" ", math.floor(#indent / 2) * 2) .. num .. ". ", text, "white", false)
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
                push(out, parse_inline(line_text, "white", false))
            end
        end
        idx = idx + 1
        ::continue::
    end

    return out
end

kkc.register_viewer_plugin({
    name = "markdown_viewer",
    version = "1.0.0",
    description = "Rendered Markdown viewer",
    modes = { "text" },
    mime_types = { "text/markdown" },
    render = render_markdown,
})
