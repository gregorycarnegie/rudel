//! Embed the Rudel spiral icon into the Windows executable so it shows in
//! Explorer and the taskbar even when the app isn't running. No-op elsewhere.

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=icon.ico");
        winresource::WindowsResource::new()
            .set_icon("icon.ico")
            .compile()
            .expect("embed icon.ico");
    }
}
