use super::MaskKind;

pub(super) fn mask_keywords(mask: MaskKind) -> &'static [&'static str] {
    match mask {
        MaskKind::C => &[
            "asm", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
            "enum", "extern", "float", "for", "goto", "if", "int", "long", "register", "return",
            "short", "signed", "sizeof", "static", "struct", "switch", "typedef", "union",
            "unsigned", "void", "volatile", "while",
        ],
        MaskKind::Pascal => &[
            "absolute",
            "and",
            "array",
            "begin",
            "case",
            "const",
            "div",
            "do",
            "downto",
            "else",
            "end",
            "file",
            "for",
            "function",
            "goto",
            "if",
            "implementation",
            "in",
            "inline",
            "interface",
            "label",
            "mod",
            "nil",
            "not",
            "of",
            "or",
            "packed",
            "procedure",
            "program",
            "record",
            "repeat",
            "set",
            "string",
            "then",
            "to",
            "type",
            "unit",
            "until",
            "uses",
            "var",
            "while",
            "with",
            "xor",
        ],
        MaskKind::Assembler => &[
            "mov", "push", "pop", "call", "ret", "cmp", "jmp", "je", "jne", "ja", "jb", "jg", "jl",
            "add", "sub", "mul", "div", "xor", "or", "and", "lea", "int", "db", "dw", "dd", "endp",
            "ends", "assume", "xlatb", "nop",
        ],
        MaskKind::Ketchup => &[
            "blackward",
            "ketchup",
            "killers",
            "redbug",
            "access",
            "darkangel",
            "off",
            "topy",
            "kennet",
            "typeone",
            "pulpe",
            "tyby",
            "djamm",
            "vatin",
            "marjorie",
            "katana",
            "ecstasy",
            "cray",
            "magicfred",
            "cobra",
            "z",
        ],
    }
}

pub(super) fn slice_visible(s: &str, skip: usize, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut seen = 0usize;
    let mut kept = 0usize;
    for ch in s.chars() {
        if seen < skip {
            seen += 1;
            continue;
        }
        if kept >= max {
            break;
        }
        out.push(ch);
        kept += 1;
    }
    out
}

pub(super) fn pad_visible(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = s.to_string();
    let mut count = out.chars().count();
    while count < max {
        out.push(' ');
        count += 1;
    }
    out
}
