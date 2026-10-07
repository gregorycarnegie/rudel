# Native FFmpeg in Rudel

The packet send/receive, owned native resources and RGBA scaling code in
`src/lib.rs` is adapted from these upstream implementations, reduced to the
video playback Rudel needs:

- [ffmpeg-next](https://github.com/zmwangx/rust-ffmpeg/tree/e2988eb8900f262a61545f0a8835b1cae2155c41),
  by meh. and Zhiming Wang: `src/codec/decoder/opened.rs`,
  `src/software/scaling/context.rs`, and `examples/dump-frames.rs`.
  Its WTFPL licence is in `LICENSE.ffmpeg-next`.
- [rsmpeg](https://github.com/larksuite/rsmpeg/tree/b21fcfde8bb1ffdc179504e370e330385baa9819),
  by the rsmpeg authors: `src/avcodec/codec.rs` and
  `src/avformat/avformat.rs`. Its MIT licence is in `LICENSE.rsmpeg`.

The FFI declarations are generated from the actual FFmpeg 9 headers at build
time. Neither wrapper crate is a dependency; there is no FFmpeg subprocess or
runtime download. The native libraries are statically linked, with dav1d for
AV1, zlib, and HTTPS through Windows SChannel or OpenSSL on Linux/macOS.

`vcpkg.json` pins FFmpeg 9.0.2 and the complete native dependency baseline.
`tools/build-ffmpeg.py` builds it; the pinned vcpkg ports contain the source
checksums, patches and configuration. GPL/nonfree FFmpeg components are not
enabled. Native dependency licences accompany release archives in `licenses/`.
Rudel's own code remains AGPL-3.0-or-later.
