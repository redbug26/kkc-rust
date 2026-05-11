// Nerd Font icon mapping for panel entries.
// Lookups are name/extension based only, so listing a directory never opens files just to draw icons.

pub const FOLDER: &str = "\u{e5ff}";
pub const FOLDER_CONFIG: &str = "\u{e5fc}";
pub const FILE: &str = "\u{f15b}";
pub const ARCHIVE: &str = "\u{f410}";
pub const AUDIO: &str = "\u{f1c7}";
pub const IMAGE: &str = "\u{f1c5}";
pub const PDF: &str = "\u{f1c1}";
pub const VIDEO: &str = "\u{f1c8}";

const EXTENSION_ICONS: &[(&str, &str)] = &[
    (".7z", ARCHIVE),
    (".aac", AUDIO),
    (".accdb", "\u{e706}"),
    (".aiff", AUDIO),
    (".ayt", AUDIO),
    (".arj", ARCHIVE),
    (".asc", "\u{f084}"),
    (".asm", "\u{e6ab}"),
    (".avi", VIDEO),
    (".bash", "\u{f489}"),
    (".bat", "\u{e629}"),
    (".bicep", "\u{e63b}"),
    (".bmp", IMAGE),
    (".bz2", ARCHIVE),
    (".c", "\u{f0671}"),
    (".cab", ARCHIVE),
    (".cer", "\u{f0a3}"),
    (".cert", "\u{f0a3}"),
    (".cfg", "\u{f013}"),
    (".class", "\u{e256}"),
    (".clj", "\u{e768}"),
    (".cljs", "\u{e768}"),
    (".cmd", "\u{e629}"),
    (".conf", "\u{f013}"),
    (".config", "\u{f013}"),
    (".cpp", "\u{f0672}"),
    (".crt", "\u{f0a3}"),
    (".cs", "\u{f031b}"),
    (".css", "\u{e749}"),
    (".csv", "\u{f021b}"),
    (".dart", "\u{e798}"),
    (".db", "\u{e64d}"),
    (".deb", "\u{f03d6}"),
    (".dll", "\u{f187}"),
    (".doc", "\u{f022c}"),
    (".docx", "\u{f022c}"),
    (".dsk", ARCHIVE),
    (".eml", "\u{f0e0}"),
    (".erl", "\u{e7b1}"),
    (".err", "\u{f03a}"),
    (".exe", "\u{f08c6}"),
    (".ex", "\u{e62d}"),
    (".exs", "\u{e62d}"),
    (".fish", "\u{f489}"),
    (".flac", AUDIO),
    (".flv", VIDEO),
    (".fs", "\u{e7a7}"),
    (".gif", IMAGE),
    (".go", "\u{e724}"),
    (".gpg", "\u{f084}"),
    (".gradle", "\u{f07c6}"),
    (".gz", ARCHIVE),
    (".h", "\u{f0671}"),
    (".hpp", "\u{f0672}"),
    (".hs", "\u{e777}"),
    (".htm", "\u{e60e}"),
    (".html", "\u{e60e}"),
    (".ico", IMAGE),
    (".ini", "\u{f013}"),
    (".ipynb", "\u{f082e}"),
    (".jar", "\u{e256}"),
    (".java", "\u{e256}"),
    (".jl", "\u{e624}"),
    (".jpeg", IMAGE),
    (".jpg", IMAGE),
    (".js", "\u{e74e}"),
    (".json", "\u{e60b}"),
    (".jsx", "\u{e7ba}"),
    (".key", "\u{f084}"),
    (".kt", "\u{e634}"),
    (".less", "\u{e758}"),
    (".lock", "\u{f023}"),
    (".log", "\u{f03a}"),
    (".lua", "\u{e620}"),
    (".m4a", AUDIO),
    (".markdown", "\u{e73e}"),
    (".md", "\u{e73e}"),
    (".mdb", "\u{e706}"),
    (".mjs", "\u{e74e}"),
    (".mkv", VIDEO),
    (".mov", VIDEO),
    (".mp3", AUDIO),
    (".mp4", VIDEO),
    (".mpeg", VIDEO),
    (".mpg", VIDEO),
    (".msi", "\u{f03d6}"),
    (".ogg", AUDIO),
    (".opus", AUDIO),
    (".otf", "\u{f031}"),
    (".pdf", PDF),
    (".pem", "\u{f084}"),
    (".php", "\u{e73d}"),
    (".pl", "\u{e769}"),
    (".png", IMAGE),
    (".ppt", "\u{f0227}"),
    (".pptx", "\u{f0227}"),
    (".ps1", "\u{f07b7}"),
    (".pub", "\u{f084}"),
    (".py", "\u{e606}"),
    (".r", "\u{f07d4}"),
    (".rar", ARCHIVE),
    (".rb", "\u{f43b}"),
    (".rpm", "\u{f03d6}"),
    (".rs", "\u{e7a8}"),
    (".rst", "\u{e73e}"),
    (".rtf", "\u{f022c}"),
    (".sass", "\u{e74b}"),
    (".scala", "\u{e737}"),
    (".scss", "\u{e74b}"),
    (".sh", "\u{f489}"),
    (".sql", "\u{e706}"),
    (".sqlite", "\u{e706}"),
    (".svg", "\u{f0721}"),
    (".swift", "\u{e699}"),
    (".tar", ARCHIVE),
    (".tf", "\u{e69a}"),
    (".tfvars", "\u{e69a}"),
    (".tgz", ARCHIVE),
    (".tif", IMAGE),
    (".tiff", IMAGE),
    (".toml", "\u{f013}"),
    (".ts", "\u{e628}"),
    (".tsx", "\u{e7ba}"),
    (".tsv", "\u{f021b}"),
    (".ttf", "\u{f031}"),
    (".txt", "\u{f0219}"),
    (".vue", "\u{f0844}"),
    (".wav", AUDIO),
    (".webm", VIDEO),
    (".webp", IMAGE),
    (".wma", AUDIO),
    (".woff", "\u{f031}"),
    (".woff2", "\u{f031}"),
    (".xls", "\u{f021b}"),
    (".xlsx", "\u{f021b}"),
    (".xml", "\u{f05c0}"),
    (".xz", ARCHIVE),
    (".yaml", "\u{f0262}"),
    (".yml", "\u{f0262}"),
    (".zip", ARCHIVE),
    (".zsh", "\u{f489}"),
    (".zst", ARCHIVE),
];

const FILE_NAME_ICONS: &[(&str, &str)] = &[
    ("cargo.lock", "\u{e7a8}"),
    ("cargo.toml", "\u{e7a8}"),
    ("dockerfile", "\u{e7b0}"),
    ("makefile", "\u{e673}"),
    ("package.json", "\u{e616}"),
    ("readme", "\u{e73e}"),
    ("readme.md", "\u{e73e}"),
];

const WELL_KNOWN_DIR_ICONS: &[(&str, &str)] = &[
    (".aws", "\u{e7ad}"),
    (".azure", "\u{f0805}"),
    (".cache", "\u{f00e8}"),
    (".cargo", FOLDER_CONFIG),
    (".config", "\u{e615}"),
    (".docker", "\u{e7b0}"),
    (".git", "\u{e65d}"),
    (".github", "\u{e65b}"),
    (".kube", "\u{f0833}"),
    (".vscode", "\u{e8da}"),
    ("apps", "\u{f003b}"),
    ("applications", "\u{f003b}"),
    ("assets", "\u{f024f}"),
    ("bin", "\u{f471}"),
    ("build", "\u{eb9d}"),
    ("desktop", "\u{f07c0}"),
    ("dist", "\u{eb9d}"),
    ("doc", "\u{f401}"),
    ("docs", "\u{f401}"),
    ("documents", "\u{f401}"),
    ("downloads", "\u{f024d}"),
    ("fonts", "\u{f031}"),
    ("images", "\u{f024f}"),
    ("img", "\u{f024f}"),
    ("lib", "\u{ebdf}"),
    ("libs", "\u{ebdf}"),
    ("media", "\u{f40f}"),
    ("movies", "\u{f0381}"),
    ("music", "\u{f0333}"),
    ("node_modules", "\u{e616}"),
    ("out", "\u{eb9d}"),
    ("packages", "\u{e616}"),
    ("photos", "\u{f024f}"),
    ("pictures", "\u{f024f}"),
    ("projects", "\u{e601}"),
    ("scripts", "\u{e691}"),
    ("src", "\u{f489}"),
    ("test", "\u{f0668}"),
    ("tests", "\u{f0668}"),
    ("videos", "\u{f0381}"),
];

pub fn icon_for_entry(name: &str, is_dir: bool) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();

    if is_dir {
        return WELL_KNOWN_DIR_ICONS
            .iter()
            .find_map(|(key, icon)| (*key == lower).then_some(*icon))
            .or(Some(FOLDER));
    }

    if let Some(icon) = FILE_NAME_ICONS
        .iter()
        .find_map(|(key, icon)| (*key == lower).then_some(*icon))
    {
        return Some(icon);
    }

    EXTENSION_ICONS
        .iter()
        .filter(|(ext, _)| lower.ends_with(*ext))
        .max_by_key(|(ext, _)| ext.len())
        .map(|(_, icon)| *icon)
        .or(Some(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_names_use_specialized_icons_or_folder_fallback() {
        assert_eq!(icon_for_entry("src", true), Some("\u{f489}"));
        assert_eq!(icon_for_entry("anything", true), Some(FOLDER));
    }

    #[test]
    fn file_names_and_extensions_are_case_insensitive() {
        assert_eq!(icon_for_entry("Cargo.toml", false), Some("\u{e7a8}"));
        assert_eq!(icon_for_entry("photo.JPG", false), Some(IMAGE));
        assert_eq!(icon_for_entry("archive.tar.gz", false), Some(ARCHIVE));
    }
}
