local kkc             = require("kkc")

local MAX_COL_WIDTH   = 30
local MAX_TOTAL_WIDTH = 120 -- max rendered line width; reduce to taste
local MAX_ROWS        = 500

-- Count unquoted occurrences of a separator character in a string
local function count_unquoted(str, sep)
    local count = 0
    local in_quotes = false
    for idx = 1, #str do
        local ch = str:sub(idx, idx)
        if ch == '"' then
            in_quotes = not in_quotes
        elseif not in_quotes and ch == sep then
            count = count + 1
        end
    end
    return count
end

-- Detect the most likely field separator from the first line
local function detect_separator(first_line)
    local candidates = { ",", ";", "\t", "|" }
    local best, best_count = ",", 0
    for _, sep in ipairs(candidates) do
        local n = count_unquoted(first_line, sep)
        if n > best_count then
            best, best_count = sep, n
        end
    end
    return best
end

-- Parse one CSV line into a list of field strings
local function parse_line(line, sep)
    local fields = {}
    local idx    = 1
    local len    = #line

    while idx <= len do
        if line:sub(idx, idx) == '"' then
            -- Quoted field
            idx = idx + 1
            local buf = {}
            while idx <= len do
                local ch = line:sub(idx, idx)
                if ch == '"' then
                    if line:sub(idx + 1, idx + 1) == '"' then
                        table.insert(buf, '"')
                        idx = idx + 2
                    else
                        idx = idx + 1
                        break
                    end
                else
                    table.insert(buf, ch)
                    idx = idx + 1
                end
            end
            table.insert(fields, table.concat(buf))
            if line:sub(idx, idx) == sep then
                idx = idx + 1
            end
        else
            -- Unquoted field
            local start = idx
            while idx <= len and line:sub(idx, idx) ~= sep do
                idx = idx + 1
            end
            table.insert(fields, line:sub(start, idx - 1))
            if idx <= len then
                idx = idx + 1
            end
        end
    end

    -- Trailing separator → empty last field
    if len > 0 and line:sub(len, len) == sep then
        table.insert(fields, "")
    end

    return fields
end

-- UTF-8-aware string length
local function text_len(s)
    return utf8.len(s) or #s
end

-- Truncate to at most max_chars display characters, appending "…" if cut
local function truncate(s, max_chars)
    local len = utf8.len(s)
    if not len or len <= max_chars then
        return s
    end
    local byte_end = utf8.offset(s, max_chars)
    if byte_end then
        return s:sub(1, byte_end - 1) .. "…"
    end
    return s:sub(1, max_chars - 1) .. "…"
end

local function pad_right(s, width)
    local gap = width - text_len(s)
    if gap <= 0 then return s end
    return s .. string.rep(" ", gap)
end

local function pad_left(s, width)
    local gap = width - text_len(s)
    if gap <= 0 then return s end
    return string.rep(" ", gap) .. s
end

local function is_numeric(s)
    return s ~= "" and (
        s:match("^%-?%d+%.?%d*$") ~= nil or
        s:match("^%-?%.%d+$") ~= nil or
        s:match("^%-?%d+[eE][+-]?%d+$") ~= nil
    )
end

-- Reduce column widths so total rendered width fits within `max_w`.
-- The separator between columns is "  │  " = 5 chars.
local function fit_widths(widths, max_w)
    local SEP = 5
    local n   = #widths
    max_w     = max_w or MAX_TOTAL_WIDTH

    local function total(list)
        local t = n > 1 and (n - 1) * SEP or 0
        for _, w in ipairs(list) do t = t + w end
        return t
    end

    if total(widths) <= max_w then
        return widths
    end

    -- Work on a copy
    local r = {}
    for i, w in ipairs(widths) do r[i] = w end

    -- Reduce the widest column by the minimum amount needed to reach the
    -- next widest, then repeat.  This distributes cuts across wide columns.
    while total(r) > max_w do
        -- find the two largest widths
        local m1, m1i = 0, 1
        local m2      = 0
        for i, w in ipairs(r) do
            if w > m1 then
                m2, m1, m1i = m1, w, i
            elseif w > m2 then
                m2 = w
            end
        end
        if m1 <= 1 then break end -- cannot reduce further
        -- reduce by at most (m1 - m2) chars, but only as much as needed
        local step   = math.max(1, m1 - m2)
        local excess = total(r) - max_w
        r[m1i]       = math.max(1, m1 - math.min(step, excess))
    end

    return r
end

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

local function render_csv(path, mode, state, width)
    if mode ~= "text" then
        return nil
    end
    local ext = path:match("%.([^%.\\/]+)$")
    if not ext or ext:lower() ~= "csv" then
        return nil
    end

    -- Use the actual panel width passed from the host; fall back to MAX_TOTAL_WIDTH.
    local max_width = (tonumber(width) or 0)
    if max_width < 20 then max_width = MAX_TOTAL_WIDTH end

    -- Read sort state (passed from previous handle_key calls)
    state           = state or {}
    local sort_col  = tonumber(state.sort_col) or 0 -- 0 = no sort
    local sort_dir  = state.sort_dir or "asc"

    -- Read file line by line, capping at MAX_ROWS + header
    local file, err = io.open(path, "r")
    if not file then
        return { { span("Error opening file: " .. tostring(err), "red") } }
    end

    local raw_lines  = {}
    local total_rows = 0
    for raw in file:lines() do
        raw = raw:gsub("\r$", "")
        total_rows = total_rows + 1
        if total_rows <= MAX_ROWS + 1 then
            table.insert(raw_lines, raw)
        end
    end
    file:close()

    if #raw_lines == 0 then
        return { { span("(empty file)", "gray") } }
    end

    local sep       = detect_separator(raw_lines[1])
    local sep_label = sep == "\t" and "TAB" or sep == "|" and "|" or sep

    -- Parse all rows
    local rows      = {}
    for _, raw in ipairs(raw_lines) do
        table.insert(rows, parse_line(raw, sep))
    end

    -- Determine column count, widths, and numeric flags
    local col_count = 0
    for _, row in ipairs(rows) do
        if #row > col_count then col_count = #row end
    end

    -- Clamp sort_col to valid range
    sort_col          = math.min(sort_col, col_count)

    local col_widths  = {}
    local col_numeric = {}
    for col = 1, col_count do
        col_widths[col]  = 0
        col_numeric[col] = true
    end

    for row_idx, row in ipairs(rows) do
        for col = 1, #row do
            local val = row[col] or ""
            local w   = math.min(text_len(val), MAX_COL_WIDTH)
            if w > col_widths[col] then
                col_widths[col] = w
            end
            -- Numeric detection skips the header row
            if row_idx > 1 and val ~= "" and not is_numeric(val) then
                col_numeric[col] = false
            end
        end
    end

    -- Reserve room for sort indicator "▲"/"▼" in the active column header
    if sort_col > 0 and col_widths[sort_col] then
        col_widths[sort_col] = math.min(col_widths[sort_col] + 2, MAX_COL_WIDTH)
    end

    -- Shrink column widths so the total line width fits the panel width
    col_widths = fit_widths(col_widths, max_width)

    -- Sort data rows (keep header at position 1)
    if sort_col > 0 and #rows > 1 then
        local header = rows[1]
        local data   = {}
        for i = 2, #rows do
            data[i - 1] = rows[i]
        end
        local numeric_sort = col_numeric[sort_col]
        table.sort(data, function(a, b)
            local va = a[sort_col] or ""
            local vb = b[sort_col] or ""
            if numeric_sort then
                local na, nb = tonumber(va), tonumber(vb)
                if na and nb then
                    -- Explicit if/else avoids operator-precedence pitfalls
                    if sort_dir == "asc" then
                        return na < nb
                    else
                        return na > nb
                    end
                end
                -- fall through to string comparison for mixed cells
            end
            if sort_dir == "asc" then
                return va < vb
            else
                return va > vb
            end
        end)
        rows = { header }
        for _, r in ipairs(data) do
            table.insert(rows, r)
        end
    end

    -- Build output
    local out       = {}
    local data_rows = math.max(0, #rows - 1)

    -- Summary/hint line
    local sort_hint
    if sort_col > 0 then
        local arrow = sort_dir == "asc" and "▲" or "▼"
        sort_hint = string.format("col %d %s", sort_col, arrow)
    else
        sort_hint = "none"
    end
    table.insert(out, {
        span("CSV", "yellow", true),
        span("  sep: ", "gray"),
        span(sep_label, "cyan"),
        span("  cols: ", "gray"),
        span(tostring(col_count), "cyan"),
        span("  rows: ", "gray"),
        span(tostring(data_rows), "cyan"),
        total_rows > MAX_ROWS + 1
        and span(string.format("  (first %d)", MAX_ROWS), "yellow")
        or span("", "gray"),
        span("  sort: ", "gray"),
        span(sort_hint, "lightyellow"),
        span("  [< >] col  [s] dir", "darkgray"),
    })
    table.insert(out, { span("") })

    for row_idx, row in ipairs(rows) do
        local is_header = row_idx == 1
        local spans     = {}

        for col = 1, col_count do
            local val         = row[col] or ""
            local width       = col_widths[col]
            local is_sort     = col == sort_col

            -- Add sort indicator to the header of the active sort column
            local display_val = val
            if is_header and is_sort then
                local arrow = sort_dir == "asc" and " ▲" or " ▼"
                display_val = val .. arrow
            end
            local display = truncate(display_val, width)

            -- Pad: numeric columns right-aligned, text left-aligned
            local padded
            if col_numeric[col] then
                padded = pad_left(display, width)
            else
                padded = pad_right(display, width)
            end

            if col > 1 then
                table.insert(spans, span("  │  ", "gray"))
            end

            local fg
            if is_header and is_sort then
                fg = "lightyellow"
            elseif is_header then
                fg = "lightcyan"
            elseif col_numeric[col] then
                fg = "lightgreen"
            else
                fg = "white"
            end

            table.insert(spans, span(padded, fg, is_header))
        end

        table.insert(out, spans)

        -- Separator line under the header
        if is_header then
            local sep_spans = {}
            for col = 1, col_count do
                if col > 1 then
                    table.insert(sep_spans, span("──┼──", "gray"))
                end
                table.insert(sep_spans, span(string.rep("─", col_widths[col]), "gray"))
            end
            table.insert(out, sep_spans)
        end
    end

    return out
end

local function handle_csv_key(path, mode, key, state)
    if mode ~= "text" then
        return nil
    end
    local ext = path:match("%.([^%.\\/]+)$")
    if not ext or ext:lower() ~= "csv" then
        return nil
    end

    state           = state or {}
    local sort_col  = tonumber(state.sort_col) or 0
    local sort_dir  = state.sort_dir or "asc"

    -- Determine column count from the first line of the file
    local col_count = 0
    local f         = io.open(path, "r")
    if f then
        local first = f:read("*l")
        f:close()
        if first then
            local sep = detect_separator(first)
            col_count = #parse_line(first, sep)
        end
    end

    local consumed = false
    if key == "char:<" then
        if sort_col > 1 then
            sort_col = sort_col - 1
            consumed = true
        elseif sort_col == 1 then
            sort_col = 0  -- clear sort
            consumed = true
        end
    elseif key == "char:>" then
        if col_count > 0 then
            sort_col = math.min(sort_col + 1, col_count)
            consumed = true
        end
    elseif key == "char:s" then
        if sort_col > 0 then
            sort_dir = sort_dir == "asc" and "desc" or "asc"
            consumed = true
        end
    end

    return {
        consumed = consumed,
        state    = {
            sort_col = tostring(sort_col),
            sort_dir = sort_dir,
        },
    }
end

kkc.register_viewer_plugin({
    name        = "csv_viewer",
    description = "CSV file viewer with column alignment",
    modes       = { "text" },
    render      = render_csv,
    handle_key  = handle_csv_key,
})
