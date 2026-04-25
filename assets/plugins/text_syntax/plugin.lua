local kkc = require("kkc")

local keyword_sets = {
    rust = {
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
        "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
        "trait", "true", "type", "unsafe", "use", "where", "while",
    },
    lua = {
        "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto",
        "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true",
        "until", "while",
    },
    c = {
        "auto", "break", "case", "char", "const", "continue", "default", "do", "double",
        "else", "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long",
        "register", "return", "short", "signed", "sizeof", "static", "struct", "switch",
        "typedef", "union", "unsigned", "void", "volatile", "while",
    },
    js = {
        "async", "await", "break", "case", "catch", "class", "const", "continue", "debugger",
        "default", "delete", "do", "else", "export", "extends", "false", "finally", "for",
        "function", "if", "import", "in", "instanceof", "let", "new", "null", "return",
        "super", "switch", "this", "throw", "true", "try", "typeof", "undefined", "var",
        "void", "while", "with", "yield",
    },
}

local extension_language = {
    rs = "rust",
    lua = "lua",
    c = "c",
    h = "c",
    cpp = "c",
    hpp = "c",
    js = "js",
    ts = "js",
    json = "js",
}

local function keyword_map(language)
    local out = {}
    for _, word in ipairs(keyword_sets[language] or keyword_sets.c) do
        out[word] = true
    end
    return out
end

local function language_for(path)
    local ext = path:match("%.([^%.\\/]+)$")
    if not ext then
        return "c"
    end
    return extension_language[ext:lower()] or "c"
end

local function push(spans, text, fg, bold)
    if text ~= "" then
        table.insert(spans, { text = text, fg = fg, bg = "black", bold = bold or false })
    end
end

local function highlight_line(line, language)
    local keywords = keyword_map(language)
    local spans = {}
    local idx = 1

    while idx <= #line do
        local two = line:sub(idx, idx + 1)
        if two == "//" or two == "--" then
            push(spans, line:sub(idx), "green")
            break
        end

        local ch = line:sub(idx, idx)
        if ch == '"' or ch == "'" then
            local quote = ch
            local start = idx
            idx = idx + 1
            while idx <= #line do
                local cur = line:sub(idx, idx)
                if cur == "\\" then
                    idx = idx + 2
                elseif cur == quote then
                    idx = idx + 1
                    break
                else
                    idx = idx + 1
                end
            end
            push(spans, line:sub(start, idx - 1), "magenta")
        elseif ch:match("[%a_]") then
            local start = idx
            idx = idx + 1
            while idx <= #line and line:sub(idx, idx):match("[%w_]") do
                idx = idx + 1
            end
            local token = line:sub(start, idx - 1)
            if keywords[token] then
                push(spans, token, "yellow", true)
            else
                push(spans, token, "white")
            end
        elseif ch:match("%d") then
            local start = idx
            idx = idx + 1
            while idx <= #line and line:sub(idx, idx):match("[%w%.]") do
                idx = idx + 1
            end
            push(spans, line:sub(start, idx - 1), "cyan")
        else
            push(spans, ch, "white")
            idx = idx + 1
        end
    end

    return spans
end

kkc.register_viewer_plugin({
    name = "text_syntax",
    description = "Syntax highlighting for text files",
    modes = { "text" },
    render_line = function(path, mode, line)
        if mode ~= "text" and mode ~= "ansi" then
            return nil
        end
        return highlight_line(line, language_for(path))
    end,
})
