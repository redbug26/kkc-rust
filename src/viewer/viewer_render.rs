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
