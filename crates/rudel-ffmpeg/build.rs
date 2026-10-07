use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../vcpkg.json");
    println!("cargo:rerun-if-changed=../../tools/build-ffmpeg.py");
    let target = env::var("TARGET").unwrap();
    let triplet = match target.as_str() {
        "x86_64-pc-windows-msvc" => "x64-windows-static-md",
        "x86_64-unknown-linux-gnu" => "x64-linux",
        "aarch64-unknown-linux-gnu" => "arm64-linux",
        "aarch64-apple-darwin" => "arm64-osx",
        "x86_64-apple-darwin" => "x64-osx",
        _ => panic!("unsupported FFmpeg target: {target}"),
    };
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../target/ffmpeg")
        .join(triplet);
    let include = root.join("include");
    assert!(
        include.join("libavformat/avformat.h").exists(),
        "Build the bundled FFmpeg first: python tools/build-ffmpeg.py"
    );
    let pkgconf = root.join("tools/pkgconf").join(if cfg!(windows) {
        "pkgconf.exe"
    } else {
        "pkgconf"
    });
    // This build script is single-threaded; set these before starting bindgen.
    unsafe {
        env::set_var("PKG_CONFIG", pkgconf);
        env::set_var("PKG_CONFIG_LIBDIR", root.join("lib/pkgconfig"));
        env::set_var("PKG_CONFIG_PATH", root.join("lib/pkgconfig"));
    }
    for (library, major) in [("libswscale", "10"), ("libavformat", "63")] {
        pkg_config::Config::new()
            .statik(true)
            .atleast_version(major)
            .probe(library)
            .unwrap_or_else(|e| panic!("bundled {library}: {e}"));
    }
    bindgen::Builder::default()
        .header_contents("rudel.h", r#"
            #include <errno.h>
            #include <libavcodec/avcodec.h>
            #include <libavformat/avformat.h>
            #include <libswscale/swscale.h>
            #if LIBAVCODEC_VERSION_MAJOR != 63 || LIBAVFORMAT_VERSION_MAJOR != 63
            #error Rudel requires the pinned FFmpeg 9 libraries
            #endif
            enum { RUDEL_EOF = AVERROR_EOF, RUDEL_AGAIN = AVERROR(EAGAIN),
                   RUDEL_BILINEAR = SWS_BILINEAR };
            static const int64_t RUDEL_NOPTS = AV_NOPTS_VALUE;
        "#)
        .clang_arg(format!("-I{}", include.display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_function("av(format_.*|codec_.*|_find_best_stream|_read_frame|_seek_frame|_packet_.*|_frame_.*|_dict_.*|_strerror|_guess_frame_rate)")
        .allowlist_function("sws_(alloc_context|scale_frame|free_context)")
        .allowlist_var("AV(SEEK_FLAG_BACKWARD|_NOPTS_VALUE)|SWS_BILINEAR|RUDEL_.*")
        .prepend_enum_name(false)
        .derive_debug(false)
        .layout_tests(false)
        .generate_comments(false)
        .formatter(bindgen::Formatter::None)
        .generate()
        .expect("generate FFmpeg bindings (libclang is required)")
        .write_to_file(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("ffmpeg.rs"))
        .unwrap();
}
