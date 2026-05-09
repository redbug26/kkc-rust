local kkc = require("kkc")

local MAX_LINES = 20000
local MAX_INPUT_BYTES = 8 * 1024 * 1024
local PREVIEW_INPUT_BYTES = 256 * 1024
local MAX_TOKENS = MAX_LINES * 2
local PREVIEW_MAX_TOKENS = 3000

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

local function text_len(text)
    if text == "" then
        return 0
    end
    return utf8.len(text) or #text
end

local function sub_chars(text, first_char, count)
    if count <= 0 then
        return ""
    end
    local first = utf8.offset(text, first_char)
    if not first then
        return ""
    end
    local after = utf8.offset(text, first_char + count)
    if after then
        return text:sub(first, after - 1)
    end
    return text:sub(first)
end

local function clone_span(item, text)
    return {
        text = text,
        fg = item.fg,
        bg = item.bg,
        bold = item.bold,
    }
end

local function push_line(lines, spans)
    if #lines < MAX_LINES then
        table.insert(lines, spans)
    end
end

local function push_wrapped(lines, spans, width, wrap)
    width = math.max(width or 80, 20)
    if not wrap then
        push_line(lines, spans)
        return
    end

    local current = {}
    local used = 0
    for _, item in ipairs(spans) do
        local text = item.text or ""
        local idx = 1
        local len = text_len(text)
        while idx <= len do
            local free = width - used
            if free <= 0 then
                push_line(lines, current)
                current = {}
                used = 0
                free = width
            end

            local take = math.min(free, len - idx + 1)
            local chunk = sub_chars(text, idx, take)
            table.insert(current, clone_span(item, chunk))
            used = used + text_len(chunk)
            idx = idx + take

            if used >= width and idx <= len then
                push_line(lines, current)
                current = {}
                used = 0
            end
        end
    end

    if #current > 0 or #spans == 0 then
        push_line(lines, current)
    end
end

local function read_limited(path, max_bytes)
    local file, err = io.open(path, "rb")
    if not file then
        return nil, err
    end

    local size = nil
    if file:seek("end") then
        size = file:seek()
        file:seek("set")
    end

    max_bytes = max_bytes or MAX_INPUT_BYTES
    local read_size = size and math.min(size, max_bytes) or max_bytes
    local data = file:read(read_size)
    file:close()
    if not data then
        return ""
    end
    return data, nil, size and size > #data, size
end

local function trim(text)
    return (text:gsub("^%s+", ""):gsub("%s+$", ""))
end

local function normalize_space(text)
    return trim((text:gsub("%s+", " ")))
end

local function starts_with(text, pos, prefix)
    return text:sub(pos, pos + #prefix - 1) == prefix
end

local function strip_bom(text)
    if text:sub(1, 3) == "\239\187\191" then
        return text:sub(4)
    end
    return text
end

local function find_markup_end(input, start_pos)
    local quote = nil
    local idx = start_pos
    while idx <= #input do
        local ch = input:sub(idx, idx)
        if quote then
            if ch == quote then
                quote = nil
            end
        elseif ch == '"' or ch == "'" then
            quote = ch
        elseif ch == ">" then
            return idx
        end
        idx = idx + 1
    end
    return nil
end

local function parse_attributes(source)
    local attrs = {}
    local idx = 1
    while idx <= #source do
        while source:sub(idx, idx):match("%s") do
            idx = idx + 1
        end
        if idx > #source then
            break
        end

        local name_start = idx
        while idx <= #source and not source:sub(idx, idx):match("[%s=]") do
            idx = idx + 1
        end
        local name = source:sub(name_start, idx - 1)
        while idx <= #source and source:sub(idx, idx):match("%s") do
            idx = idx + 1
        end

        local value = nil
        if source:sub(idx, idx) == "=" then
            idx = idx + 1
            while idx <= #source and source:sub(idx, idx):match("%s") do
                idx = idx + 1
            end
            local quote = source:sub(idx, idx)
            if quote == '"' or quote == "'" then
                idx = idx + 1
                local value_start = idx
                while idx <= #source and source:sub(idx, idx) ~= quote do
                    idx = idx + 1
                end
                value = source:sub(value_start, idx - 1)
                if source:sub(idx, idx) == quote then
                    idx = idx + 1
                end
            else
                local value_start = idx
                while idx <= #source and not source:sub(idx, idx):match("%s") do
                    idx = idx + 1
                end
                value = source:sub(value_start, idx - 1)
            end
        end

        if name ~= "" then
            table.insert(attrs, { name = name, value = value })
        end
    end
    return attrs
end

local function parse_tag(inner)
    local raw = trim(inner)
    if raw == "" then
        return { kind = "error", message = "empty tag" }
    end

    if raw:sub(1, 1) == "/" then
        local name = trim(raw:sub(2)):match("^([^%s]+)") or ""
        return { kind = "close", name = name }
    end

    local self_closing = raw:match("/%s*$") ~= nil
    if self_closing then
        raw = trim(raw:gsub("/%s*$", ""))
    end

    local name, rest = raw:match("^([^%s]+)%s*(.-)$")
    if not name or name == "" then
        return { kind = "error", message = "missing tag name" }
    end

    return {
        kind = self_closing and "self" or "open",
        name = name,
        attrs = parse_attributes(rest or ""),
    }
end

local function parse_xml(input, max_tokens)
    local tokens = {}
    local errors = {}
    local pos = 1
    input = strip_bom(input)
    max_tokens = max_tokens or MAX_TOKENS

    local function push_token(token)
        if #tokens >= max_tokens then
            if #errors == 0 or errors[#errors] ~= "Stopped parsing after token limit" then
                table.insert(errors, "Stopped parsing after token limit")
            end
            return false
        end
        table.insert(tokens, token)
        return true
    end

    while pos <= #input do
        local next_lt = input:find("<", pos, true)
        if not next_lt then
            local text = normalize_space(input:sub(pos))
            if text ~= "" then
                push_token({ kind = "text", text = text })
            end
            break
        end

        if next_lt > pos then
            local text = normalize_space(input:sub(pos, next_lt - 1))
            if text ~= "" then
                if not push_token({ kind = "text", text = text }) then
                    break
                end
            end
        end

        if starts_with(input, next_lt, "<!--") then
            local end_pos = input:find("-->", next_lt + 4, true)
            if not end_pos then
                table.insert(errors, "Unclosed comment at byte " .. next_lt)
                break
            end
            if not push_token({ kind = "comment", text = input:sub(next_lt + 4, end_pos - 1) }) then
                break
            end
            pos = end_pos + 3
        elseif starts_with(input, next_lt, "<![CDATA[") then
            local end_pos = input:find("]]>", next_lt + 9, true)
            if not end_pos then
                table.insert(errors, "Unclosed CDATA at byte " .. next_lt)
                break
            end
            if not push_token({ kind = "cdata", text = input:sub(next_lt + 9, end_pos - 1) }) then
                break
            end
            pos = end_pos + 3
        elseif starts_with(input, next_lt, "<?") then
            local end_pos = input:find("?>", next_lt + 2, true)
            if not end_pos then
                table.insert(errors, "Unclosed processing instruction at byte " .. next_lt)
                break
            end
            if not push_token({ kind = "pi", text = trim(input:sub(next_lt + 2, end_pos - 1)) }) then
                break
            end
            pos = end_pos + 2
        elseif starts_with(input, next_lt, "<!") then
            local end_pos = find_markup_end(input, next_lt + 2)
            if not end_pos then
                table.insert(errors, "Unclosed declaration at byte " .. next_lt)
                break
            end
            if not push_token({ kind = "declaration", text = trim(input:sub(next_lt + 2, end_pos - 1)) }) then
                break
            end
            pos = end_pos + 1
        else
            local end_pos = find_markup_end(input, next_lt + 1)
            if not end_pos then
                table.insert(errors, "Unclosed tag at byte " .. next_lt)
                break
            end
            if not push_token(parse_tag(input:sub(next_lt + 1, end_pos - 1))) then
                break
            end
            pos = end_pos + 1
        end
    end

    return tokens, errors
end

local function attr_spans(attrs)
    local out = {}
    for _, attr in ipairs(attrs or {}) do
        table.insert(out, span(" ", "gray"))
        table.insert(out, span(attr.name, "lightyellow"))
        if attr.value ~= nil then
            table.insert(out, span("=", "gray"))
            table.insert(out, span('"' .. attr.value .. '"', "lightgreen"))
        end
    end
    return out
end

local function append_all(target, source)
    for _, item in ipairs(source) do
        table.insert(target, item)
    end
end

local function line_for_token(token, depth)
    local indent = string.rep("  ", depth)

    if token.kind == "open" or token.kind == "self" then
        local line = { span(indent, "gray"), span("<", "gray"), span(token.name, "lightcyan", true) }
        append_all(line, attr_spans(token.attrs))
        table.insert(line, span(token.kind == "self" and "/>" or ">", "gray"))
        return line
    elseif token.kind == "close" then
        return { span(indent, "gray"), span("</", "gray"), span(token.name, "lightcyan", true), span(">", "gray") }
    elseif token.kind == "text" then
        return { span(indent, "gray"), span(token.text, "white") }
    elseif token.kind == "comment" then
        return { span(indent, "gray"), span("<!-- " .. normalize_space(token.text) .. " -->", "darkgray") }
    elseif token.kind == "cdata" then
        return { span(indent, "gray"), span("<![CDATA[", "yellow", true), span(normalize_space(token.text), "white"),
            span("]]>", "yellow", true) }
    elseif token.kind == "pi" then
        return { span(indent, "gray"), span("<?", "magenta"), span(token.text, "lightmagenta"), span("?>", "magenta") }
    elseif token.kind == "declaration" then
        return { span(indent, "gray"), span("<!", "magenta"), span(token.text, "lightmagenta"), span(">", "magenta") }
    elseif token.kind == "error" then
        return { span(indent, "gray"), span("Error: " .. token.message, "red", true) }
    end

    return { span(indent, "gray"), span(token.text or "", "white") }
end

local function collect_stats(tokens, errors)
    local depth = 0
    local max_depth = 0
    local root = nil
    local node_count = 0

    for _, token in ipairs(tokens) do
        if token.kind == "open" then
            node_count = node_count + 1
            if not root then
                root = token.name
            end
            depth = depth + 1
            if depth > max_depth then
                max_depth = depth
            end
        elseif token.kind == "self" then
            node_count = node_count + 1
            if not root then
                root = token.name
            end
            if depth + 1 > max_depth then
                max_depth = depth + 1
            end
        elseif token.kind == "close" then
            depth = math.max(depth - 1, 0)
        elseif token.kind == "text" or token.kind == "comment" or token.kind == "cdata" or token.kind == "pi" or token.kind == "declaration" then
            node_count = node_count + 1
        end
    end

    return {
        root = root or "-",
        nodes = node_count,
        depth = max_depth,
        errors = #errors,
    }
end

local function render_xml(path, mode, state, width)
    if mode ~= "text" then
        return nil
    end

    state = state or {}
    local preview = state.__preview == "1"
    local state_max_bytes = tonumber(state.__preview_max_bytes)
    local max_bytes = preview and (state_max_bytes or PREVIEW_INPUT_BYTES) or MAX_INPUT_BYTES
    local max_tokens = preview and PREVIEW_MAX_TOKENS or MAX_TOKENS
    local data, err, truncated, file_size = read_limited(path, max_bytes)
    if not data then
        return {
            { span("XML: " .. tostring(err), "red", true) },
        }
    end

    local wrap = state.wrap ~= "0"
    local tokens, errors = parse_xml(data, max_tokens)
    local stats = collect_stats(tokens, errors)
    local lines = {}
    push_line(lines, {
        span("XML", "lightcyan", true),
        span("  root: ", "gray"),
        span(stats.root, "white", true),
        span("  nodes: " .. tostring(stats.nodes), "gray"),
        span("  depth: " .. tostring(stats.depth), "gray"),
        preview and span("  preview", "yellow") or span("", "gray"),
        truncated and span("  partial: " .. tostring(#data) .. "/" .. tostring(file_size) .. " bytes", "yellow") or
        span("", "gray"),
        span("  wrap: ", "gray"),
        span(wrap and "on" or "off", wrap and "lightgreen" or "yellow"),
        span("  [F2/w] wrap", "gray"),
    })

    for _, message in ipairs(errors) do
        push_wrapped(lines, { span("Error: " .. message, "red", true) }, width, wrap)
    end

    local depth = 0
    for _, token in ipairs(tokens) do
        if token.kind == "close" then
            depth = math.max(depth - 1, 0)
        end

        push_wrapped(lines, line_for_token(token, depth), width, wrap)

        if token.kind == "open" then
            depth = depth + 1
        end
    end

    return lines
end

local function handle_xml_key(_path, mode, key, state)
    if mode ~= "text" then
        return nil
    end

    state = state or {}
    if key == "f2" or key == "char:w" or key == "char:W" then
        local wrap = state.wrap ~= "0"
        return {
            consumed = true,
            state = {
                wrap = wrap and "0" or "1",
                __preview = state.__preview,
                __preview_max_bytes = state.__preview_max_bytes,
            },
        }
    end

    return { consumed = false, state = state }
end

kkc.register_viewer_plugin({
    modes = { "text" },
    render = render_xml,
    handle_key = handle_xml_key,
})
