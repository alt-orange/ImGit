//! The files under `assets/` (fonts, images, icons, cursors), compiled into the executable by `build.rs`.

/// `(path, bytes)` for every asset, sorted by path.
static ENTRIES: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/embedded_assets.rs"));

/// The bytes of the asset at `path` (relative to `assets/`, forward slashes, e.g.
/// `"fonts/OpenSans-Regular.ttf"`), or `None` when there is none. The data lives as long as the program.
#[must_use]
pub fn data(path: &str) -> Option<&'static [u8]> {
    ENTRIES
        .binary_search_by(|(name, _)| (*name).cmp(path))
        .ok()
        .map(|index| ENTRIES[index].1)
}

/// The same bytes as text; `None` as well when they are not UTF-8.
#[must_use]
pub fn text(path: &str) -> Option<&'static str> {
    data(path).and_then(|bytes| std::str::from_utf8(bytes).ok())
}

/// The path of every embedded asset, sorted.
pub fn paths() -> impl Iterator<Item = &'static str> {
    ENTRIES.iter().map(|(name, _)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_embedded_files() {
        let font = data("fonts/OpenSans-Regular.ttf").expect("the regular face is embedded");
        assert!(font.len() > 100_000);
        assert!(data("fonts/Missing.ttf").is_none());
        assert!(text("icons/fa_icons.tsv").is_some_and(|tsv| tsv.contains('\t')));
        assert!(paths().is_sorted());
    }
}
