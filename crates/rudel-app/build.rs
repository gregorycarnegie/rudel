//! Embed the Rudel spiral icon into the Windows executable so it shows in
//! Explorer and the taskbar even when the app isn't running (no-op
//! elsewhere), and embed the built-in editor themes from `themes/*.toml`.

fn main() {
    embed_themes();
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=icon.ico");
        winresource::WindowsResource::new()
            .set_icon("icon.ico")
            .compile()
            .expect("embed icon.ico");
    }
}

/// Write `$OUT_DIR/builtin_themes.rs`: every `themes/*.toml`, by file name, so
/// a theme added to the folder is built in without touching the code.
fn embed_themes() {
    let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("themes");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("themes folder")
        .map(|entry| entry.expect("theme file").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();
    let entries: String = files
        .iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            format!(
                "    ({name:?}, include_str!({:?})),\n",
                path.display().to_string()
            )
        })
        .collect();
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("builtin_themes.rs");
    std::fs::write(
        out,
        format!("pub(crate) static BUILTIN: &[(&str, &str)] = &[\n{entries}];\n"),
    )
    .expect("write builtin_themes.rs");
}
