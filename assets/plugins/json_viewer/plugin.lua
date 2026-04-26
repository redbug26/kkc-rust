local kkc = require("kkc")

local MAX_LINES = 20000

local function span(text, fg, bold)
    return { text = text, fg = fg or "white", bg = "black", bold = bold or false }
end

local function push_line(lines, spans)
    if #lines < MAX_LINES then
        table.insert(lines, spans)
    end
end

local function copy_spans(spans)
    local out = {}
    for _, item in ipairs(spans or {}) do
        table.insert(out, item)
    end
    return out
end

local function append_spans(target, source)
    for _, item in ipairs(source) do
        table.insert(target, item)
    end
end

local function is_json_path(path)
    local ext = path:match("%.([^%.\\/]+)$")
    if not ext then
        return false
    end
    ext = ext:lower()
    return ext == "json" or ext == "geojson"
end

local function read_all(path)
    local file, err = io.open(path, "rb")
    if not file then
        return nil, err
    end
    local data = file:read("*a")
    file:close()
    return data
end

local function parser_error(p, message)
    error(string.format("%s at byte %d", message, p.pos), 0)
end

local function make_parser(input)
    return {
        input = input,
        pos = 1,
        len = #input,
    }
end

local function peek(p)
    return p.input:sub(p.pos, p.pos)
end

local function skip_ws(p)
    while p.pos <= p.len do
        local ch = peek(p)
        if ch == " " or ch == "\n" or ch == "\r" or ch == "\t" then
            p.pos = p.pos + 1
        else
            break
        end
    end
end

local parse_value

local function decode_unicode_escape(p)
    local hex = p.input:sub(p.pos, p.pos + 3)
    if not hex:match("^%x%x%x%x$") then
        parser_error(p, "Invalid unicode escape")
    end
    p.pos = p.pos + 4
    local code = tonumber(hex, 16)

    if code >= 0xD800 and code <= 0xDBFF then
        if p.input:sub(p.pos, p.pos + 1) == "\\u" then
            p.pos = p.pos + 2
            local low_hex = p.input:sub(p.pos, p.pos + 3)
            if low_hex:match("^%x%x%x%x$") then
                local low = tonumber(low_hex, 16)
                if low >= 0xDC00 and low <= 0xDFFF then
                    p.pos = p.pos + 4
                    code = 0x10000 + ((code - 0xD800) * 0x400) + (low - 0xDC00)
                end
            end
        end
    end

    if utf8 and utf8.char then
        local ok, value = pcall(utf8.char, code)
        if ok then
            return value
        end
    end
    return "?"
end

local function parse_string(p)
    if peek(p) ~= '"' then
        parser_error(p, "Expected string")
    end
    p.pos = p.pos + 1
    local out = {}
    while p.pos <= p.len do
        local ch = peek(p)
        if ch == '"' then
            p.pos = p.pos + 1
            return table.concat(out)
        elseif ch == "\\" then
            p.pos = p.pos + 1
            local esc = peek(p)
            p.pos = p.pos + 1
            if esc == '"' or esc == "\\" or esc == "/" then
                table.insert(out, esc)
            elseif esc == "b" then
                table.insert(out, "\b")
            elseif esc == "f" then
                table.insert(out, "\f")
            elseif esc == "n" then
                table.insert(out, "\n")
            elseif esc == "r" then
                table.insert(out, "\r")
            elseif esc == "t" then
                table.insert(out, "\t")
            elseif esc == "u" then
                table.insert(out, decode_unicode_escape(p))
            else
                parser_error(p, "Invalid escape")
            end
        else
            if ch == "\n" or ch == "\r" then
                parser_error(p, "Unescaped newline in string")
            end
            table.insert(out, ch)
            p.pos = p.pos + 1
        end
    end
    parser_error(p, "Unterminated string")
end

local function parse_number(p)
    local start = p.pos
    if peek(p) == "-" then
        p.pos = p.pos + 1
    end
    if peek(p) == "0" then
        p.pos = p.pos + 1
    elseif peek(p):match("%d") then
        while p.pos <= p.len and peek(p):match("%d") do
            p.pos = p.pos + 1
        end
    else
        parser_error(p, "Invalid number")
    end
    if peek(p) == "." then
        p.pos = p.pos + 1
        if not peek(p):match("%d") then
            parser_error(p, "Invalid number fraction")
        end
        while p.pos <= p.len and peek(p):match("%d") do
            p.pos = p.pos + 1
        end
    end
    local e = peek(p)
    if e == "e" or e == "E" then
        p.pos = p.pos + 1
        local sign = peek(p)
        if sign == "+" or sign == "-" then
            p.pos = p.pos + 1
        end
        if not peek(p):match("%d") then
            parser_error(p, "Invalid number exponent")
        end
        while p.pos <= p.len and peek(p):match("%d") do
            p.pos = p.pos + 1
        end
    end
    return { kind = "number", value = p.input:sub(start, p.pos - 1) }
end

local function parse_literal(p, text, kind, value)
    if p.input:sub(p.pos, p.pos + #text - 1) ~= text then
        parser_error(p, "Expected " .. text)
    end
    p.pos = p.pos + #text
    return { kind = kind, value = value or text }
end

local function parse_array(p)
    p.pos = p.pos + 1
    local items = {}
    skip_ws(p)
    if peek(p) == "]" then
        p.pos = p.pos + 1
        return { kind = "array", items = items }
    end
    while true do
        table.insert(items, parse_value(p))
        skip_ws(p)
        local ch = peek(p)
        if ch == "]" then
            p.pos = p.pos + 1
            break
        elseif ch == "," then
            p.pos = p.pos + 1
            skip_ws(p)
        else
            parser_error(p, "Expected ',' or ']'")
        end
    end
    return { kind = "array", items = items }
end

local function parse_object(p)
    p.pos = p.pos + 1
    local entries = {}
    skip_ws(p)
    if peek(p) == "}" then
        p.pos = p.pos + 1
        return { kind = "object", entries = entries }
    end
    while true do
        skip_ws(p)
        local key = parse_string(p)
        skip_ws(p)
        if peek(p) ~= ":" then
            parser_error(p, "Expected ':'")
        end
        p.pos = p.pos + 1
        local value = parse_value(p)
        table.insert(entries, { key = key, value = value })
        skip_ws(p)
        local ch = peek(p)
        if ch == "}" then
            p.pos = p.pos + 1
            break
        elseif ch == "," then
            p.pos = p.pos + 1
        else
            parser_error(p, "Expected ',' or '}'")
        end
    end
    return { kind = "object", entries = entries }
end

parse_value = function(p)
    skip_ws(p)
    local ch = peek(p)
    if ch == '"' then
        return { kind = "string", value = parse_string(p) }
    elseif ch == "{" then
        return parse_object(p)
    elseif ch == "[" then
        return parse_array(p)
    elseif ch == "-" or ch:match("%d") then
        return parse_number(p)
    elseif ch == "t" then
        return parse_literal(p, "true", "boolean", "true")
    elseif ch == "f" then
        return parse_literal(p, "false", "boolean", "false")
    elseif ch == "n" then
        return parse_literal(p, "null", "null", "null")
    end
    parser_error(p, "Unexpected character")
end

local function parse_json(input)
    local p = make_parser(input)
    local root = parse_value(p)
    skip_ws(p)
    if p.pos <= p.len then
        parser_error(p, "Trailing content")
    end
    return root
end

local function escape_json_string(value)
    local out = {}
    for ch in value:gmatch(".") do
        if ch == '"' then
            table.insert(out, '\\"')
        elseif ch == "\\" then
            table.insert(out, "\\\\")
        elseif ch == "\b" then
            table.insert(out, "\\b")
        elseif ch == "\f" then
            table.insert(out, "\\f")
        elseif ch == "\n" then
            table.insert(out, "\\n")
        elseif ch == "\r" then
            table.insert(out, "\\r")
        elseif ch == "\t" then
            table.insert(out, "\\t")
        else
            table.insert(out, ch)
        end
    end
    return table.concat(out)
end

local function string_spans(value, fg)
    return {
        span('"', "gray"),
        span(escape_json_string(value), fg or "lightgreen"),
        span('"', "gray"),
    }
end

local function scalar_spans(node)
    if node.kind == "string" then
        return string_spans(node.value, "lightgreen")
    elseif node.kind == "number" then
        return { span(node.value, "lightmagenta") }
    elseif node.kind == "boolean" then
        return { span(node.value, "yellow", true) }
    elseif node.kind == "null" then
        return { span("null", "darkgray", true) }
    end
    return { span("?", "red") }
end

local function is_container(node)
    return node.kind == "object" or node.kind == "array"
end

local function render_pretty_value(lines, node, indent, prefix, trailing_comma)
    local line_prefix = copy_spans(prefix)
    if node.kind == "object" then
        table.insert(line_prefix, span("{", "gray", true))
        push_line(lines, line_prefix)
        for idx, entry in ipairs(node.entries) do
            local child_prefix = { span(string.rep(" ", indent + 2), "white") }
            append_spans(child_prefix, string_spans(entry.key, "lightcyan"))
            table.insert(child_prefix, span(": ", "gray"))
            render_pretty_value(lines, entry.value, indent + 2, child_prefix, idx < #node.entries)
        end
        local close = {
            span(string.rep(" ", indent), "white"),
            span("}", "gray", true),
        }
        if trailing_comma then
            table.insert(close, span(",", "gray"))
        end
        push_line(lines, close)
    elseif node.kind == "array" then
        table.insert(line_prefix, span("[", "gray", true))
        push_line(lines, line_prefix)
        for idx, item in ipairs(node.items) do
            local child_prefix = { span(string.rep(" ", indent + 2), "white") }
            render_pretty_value(lines, item, indent + 2, child_prefix, idx < #node.items)
        end
        local close = {
            span(string.rep(" ", indent), "white"),
            span("]", "gray", true),
        }
        if trailing_comma then
            table.insert(close, span(",", "gray"))
        end
        push_line(lines, close)
    else
        append_spans(line_prefix, scalar_spans(node))
        if trailing_comma then
            table.insert(line_prefix, span(",", "gray"))
        end
        push_line(lines, line_prefix)
    end
end

local function preview(node)
    if node.kind == "object" then
        return string.format("object (%d keys)", #node.entries), "lightblue"
    elseif node.kind == "array" then
        return string.format("array (%d items)", #node.items), "lightblue"
    elseif node.kind == "string" then
        local value = escape_json_string(node.value)
        if #value > 60 then
            value = value:sub(1, 57) .. "..."
        end
        return '"' .. value .. '"', "lightgreen"
    elseif node.kind == "number" then
        return node.value, "lightmagenta"
    elseif node.kind == "boolean" then
        return node.value, "yellow"
    elseif node.kind == "null" then
        return "null", "darkgray"
    end
    return "?", "red"
end

local function path_child(parent, key)
    if type(key) == "number" then
        return parent .. "[" .. tostring(key) .. "]"
    end
    if key:match("^[A-Za-z_][A-Za-z0-9_]*$") then
        return parent .. "." .. key
    end
    return parent .. "[" .. '"' .. escape_json_string(key) .. '"' .. "]"
end

local function render_tree_value(lines, node, path, depth)
    local text, color = preview(node)
    push_line(lines, {
        span(string.rep(" ", depth * 2), "white"),
        span(path, "lightcyan", true),
        span("  ", "gray"),
        span(text, color),
    })
    if node.kind == "object" then
        for _, entry in ipairs(node.entries) do
            render_tree_value(lines, entry.value, path_child(path, entry.key), depth + 1)
        end
    elseif node.kind == "array" then
        for idx, item in ipairs(node.items) do
            render_tree_value(lines, item, path_child(path, idx - 1), depth + 1)
        end
    end
end

local function collect_stats(node, stats, depth)
    stats.nodes = stats.nodes + 1
    if depth > stats.depth then
        stats.depth = depth
    end
    stats[node.kind] = (stats[node.kind] or 0) + 1
    if node.kind == "object" then
        for _, entry in ipairs(node.entries) do
            collect_stats(entry.value, stats, depth + 1)
        end
    elseif node.kind == "array" then
        for _, item in ipairs(node.items) do
            collect_stats(item, stats, depth + 1)
        end
    end
end

local function header_lines(path, root, input, view)
    local stats = { nodes = 0, depth = 0 }
    collect_stats(root, stats, 1)
    local name = path:match("([^\\/]+)$") or path
    return {
        {
            span("JSON", "yellow", true),
            span("  file: ", "gray"),
            span(name, "white"),
            span("  view: ", "gray"),
            span(view, "lightcyan"),
            span("  bytes: ", "gray"),
            span(tostring(#input), "lightmagenta"),
            span("  nodes: ", "gray"),
            span(tostring(stats.nodes), "lightmagenta"),
            span("  depth: ", "gray"),
            span(tostring(stats.depth), "lightmagenta"),
            span("  [p] pretty  [t] tree", "darkgray"),
        },
        { span("") },
    }
end

local function render_json(path, mode, state, width)
    if mode ~= "text" or not is_json_path(path) then
        return nil
    end

    local input, err = read_all(path)
    if not input then
        return { { span("Error opening JSON file: " .. tostring(err), "red", true) } }
    end

    local ok, root_or_error = pcall(parse_json, input)
    if not ok then
        return {
            { span("Invalid JSON", "red", true) },
            { span(tostring(root_or_error), "lightred") },
        }
    end

    state = state or {}
    local view = state.view or "pretty"
    if view ~= "tree" then
        view = "pretty"
    end

    local lines = header_lines(path, root_or_error, input, view)
    if view == "tree" then
        render_tree_value(lines, root_or_error, "$", 0)
    else
        render_pretty_value(lines, root_or_error, 0, {}, false)
    end

    if #lines >= MAX_LINES then
        table.insert(lines, { span("Output truncated at " .. tostring(MAX_LINES) .. " lines", "yellow", true) })
    end
    return lines
end

local function handle_json_key(path, mode, key, state)
    if mode ~= "text" or not is_json_path(path) then
        return nil
    end
    state = state or {}
    local view = state.view or "pretty"
    local consumed = false

    if key == "char:t" then
        view = view == "tree" and "pretty" or "tree"
        consumed = true
    elseif key == "char:p" then
        view = "pretty"
        consumed = true
    end

    return {
        consumed = consumed,
        state = {
            view = view,
        },
    }
end

kkc.register_viewer_plugin({
    name = "json_viewer",
    version = "1.0.0",
    description = "Pretty and tree JSON viewer",
    modes = { "text" },
    extensions = { "json", "geojson" },
    render = render_json,
    handle_key = handle_json_key,
})
