//! Small, owned video decoder over the FFmpeg libraries linked into Rudel.
//! The decode/scaling calls are adapted from ffmpeg-next; see NOTICE.md.

use std::{
    ffi::{CStr, CString, c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    time::Duration,
};

#[allow(warnings)]
mod ffi {
    include!(concat!(env!("OUT_DIR"), "/ffmpeg.rs"));
}
mod geometry;

use ffi::*;
use geometry::{MAX_PIXELS, Orientation, fit, frame_duration, orient, seconds};

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Presentation time in seconds in the stream's time base.
    pub timestamp: Option<f64>,
    pub duration: Duration,
}

/// Owns every native allocation; never exposes FFmpeg pointers to its caller.
/// The cancellation callback must remain valid until the decoder is dropped.
pub struct Video<'a> {
    format: *mut AVFormatContext,
    codec: *mut AVCodecContext,
    packet: *mut AVPacket,
    frame: *mut AVFrame,
    scaled: *mut AVFrame,
    scaler: *mut SwsContext,
    // A stable, thin pointer to the fat trait-object reference for C's opaque.
    #[allow(clippy::redundant_allocation)]
    interrupt: Box<&'a (dyn Fn() -> bool + Sync)>,
    stream: c_int,
    draining: bool,
    max_width: u32,
}

unsafe extern "C" fn interrupt(opaque: *mut c_void) -> c_int {
    // The box stays at the same address even when Video moves, and outlives
    // avformat_close_input. Sync also permits threaded FFmpeg protocols.
    let wanted = unsafe { &*(opaque.cast::<&(dyn Fn() -> bool + Sync)>()) };
    c_int::from(!catch_unwind(AssertUnwindSafe(wanted)).unwrap_or(false))
}

fn check(code: c_int) -> Result<c_int, String> {
    if code >= 0 {
        return Ok(code);
    }
    let mut message = [0; 256];
    unsafe {
        av_strerror(code, message.as_mut_ptr(), message.len());
        Err(format!(
            "FFmpeg: {}",
            CStr::from_ptr(message.as_ptr()).to_string_lossy()
        ))
    }
}

/// The options `avformat_open_input` is given, freed however `open` returns.
struct Options(*mut AVDictionary);

impl Options {
    fn set(&mut self, key: &CStr, value: &CStr) -> Result<(), String> {
        // FFmpeg copies both NUL-terminated strings.
        check(unsafe { av_dict_set(&mut self.0, key.as_ptr(), value.as_ptr(), 0) }).map(drop)
    }
}

impl Drop for Options {
    fn drop(&mut self) {
        unsafe { av_dict_free(&mut self.0) }
    }
}

impl<'a> Video<'a> {
    /// Open `input`: an `http(s)://` URL, or a path to a local file. Nothing
    /// else FFmpeg can open is reachable from here — no other protocol, and no
    /// protocol switch from inside a stream.
    pub fn open(
        input: &str,
        user_agent: &str,
        max_width: u32,
        wanted: &'a (dyn Fn() -> bool + Sync),
    ) -> Result<Self, String> {
        if max_width == 0 || max_width > 16384 {
            return Err("invalid video width limit".into());
        }
        let network = input.starts_with("http://") || input.starts_with("https://");
        // FFmpeg takes anything before a colon for a protocol name, so a file
        // called `take:2.mp4`, or a script's `concat:`/`subfile:` "path", would
        // open something other than the file named. `file:` says which.
        let input = CString::new(if network {
            input.to_owned()
        } else {
            format!("file:{input}")
        })
        .map_err(|_| "video path contains a NUL")?;
        let agent = CString::new(user_agent).map_err(|_| "user-agent contains a NUL")?;
        // All pointers are either null or owned, including on partial failure.
        let mut video = Self {
            format: unsafe { avformat_alloc_context() },
            codec: ptr::null_mut(),
            packet: unsafe { av_packet_alloc() },
            frame: unsafe { av_frame_alloc() },
            scaled: unsafe { av_frame_alloc() },
            scaler: unsafe { sws_alloc_context() },
            interrupt: Box::new(wanted),
            stream: 0,
            draining: false,
            max_width,
        };
        if video.format.is_null()
            || video.packet.is_null()
            || video.frame.is_null()
            || video.scaled.is_null()
            || video.scaler.is_null()
        {
            return Err("FFmpeg allocation failed".into());
        }
        let mut options = Options(ptr::null_mut());
        // Inherited by every nested open — an HLS segment, a redirect — so a
        // web stream cannot point FFmpeg at a local file, nor a file at the
        // network.
        options.set(
            c"protocol_whitelist",
            if network {
                c"http,https,httpproxy,tcp,tls,crypto"
            } else {
                c"file"
            },
        )?;
        if network {
            options.set(c"user_agent", &agent)?;
            options.set(c"tls_verify", c"1")?;
            // Static OpenSSL's compiled-in certificate directory belongs to
            // the build machine; use the operating system's CA bundle.
            #[cfg(not(windows))]
            {
                let bundle = [
                    c"/etc/ssl/cert.pem",
                    c"/etc/ssl/certs/ca-certificates.crt",
                    c"/etc/pki/tls/certs/ca-bundle.crt",
                ]
                .into_iter()
                .find(|ca| {
                    ca.to_str()
                        .is_ok_and(|ca| std::path::Path::new(ca).is_file())
                });
                if let Some(ca) = bundle {
                    options.set(c"ca_file", ca)?;
                }
            }
        }
        unsafe {
            (*video.scaler).flags = RUDEL_BILINEAR as _;
            (*video.format).interrupt_callback = AVIOInterruptCB {
                callback: Some(interrupt),
                opaque: (&mut *video.interrupt as *mut &(dyn Fn() -> bool + Sync)).cast(),
            };
            // On failure this frees the context and nulls the pointer.
            check(avformat_open_input(
                &mut video.format,
                input.as_ptr(),
                ptr::null(),
                &mut options.0,
            ))?;
            check(avformat_find_stream_info(video.format, ptr::null_mut()))?;
            let mut decoder = ptr::null();
            video.stream = check(av_find_best_stream(
                video.format,
                AVMEDIA_TYPE_VIDEO,
                -1,
                -1,
                &mut decoder,
                0,
            ))?;
            let stream = *(*video.format).streams.add(video.stream as usize);
            video.codec = avcodec_alloc_context3(decoder);
            if video.codec.is_null() {
                return Err("FFmpeg decoder allocation failed".into());
            }
            check(avcodec_parameters_to_context(
                video.codec,
                (*stream).codecpar,
            ))?;
            (*video.codec).pkt_timebase = (*stream).time_base;
            // Bound allocation on untrusted video dimensions, and avoid each
            // live source creating a decoder thread for every CPU on the host.
            (*video.codec).max_pixels = MAX_PIXELS;
            (*video.codec).thread_count = 2;
            check(avcodec_open2(video.codec, decoder, ptr::null_mut()))?;
        }
        Ok(video)
    }

    /// Read one presentation frame. None means the decoder is fully drained,
    /// including delayed B-frames; callers may then rewind it.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, String> {
        unsafe {
            loop {
                if !(self.interrupt)() {
                    return Err("video cancelled".into());
                }
                let received = avcodec_receive_frame(self.codec, self.frame);
                if received == 0 {
                    let result = self.convert();
                    av_frame_unref(self.frame);
                    return result.map(Some);
                }
                if received == RUDEL_EOF {
                    return Ok(None);
                }
                if received != RUDEL_AGAIN {
                    check(received)?;
                }
                if self.draining {
                    return Err("FFmpeg requested input after end of stream".into());
                }
                loop {
                    let read = av_read_frame(self.format, self.packet);
                    if read == RUDEL_EOF {
                        check(avcodec_send_packet(self.codec, ptr::null()))?;
                        self.draining = true;
                        break;
                    }
                    check(read)?;
                    if (*self.packet).stream_index == self.stream {
                        let sent = avcodec_send_packet(self.codec, self.packet);
                        av_packet_unref(self.packet);
                        check(sent)?;
                        break;
                    }
                    av_packet_unref(self.packet);
                }
            }
        }
    }

    pub fn rewind(&mut self) -> Result<(), String> {
        unsafe {
            let stream = *(*self.format).streams.add(self.stream as usize);
            let start = (*stream).start_time;
            check(av_seek_frame(
                self.format,
                self.stream,
                if start == RUDEL_NOPTS { 0 } else { start },
                AVSEEK_FLAG_BACKWARD as c_int,
            ))?;
            avcodec_flush_buffers(self.codec);
            av_packet_unref(self.packet);
            av_frame_unref(self.frame);
            self.draining = false;
        }
        Ok(())
    }

    fn convert(&mut self) -> Result<Frame, String> {
        // Frame planes/strides come from FFmpeg, and remain alive throughout
        // scaling/copying. Output is an FFmpeg allocation too: its SIMD paths
        // require alignment/padding that a tightly packed Vec does not promise.
        unsafe {
            let frame = &*self.frame;
            if frame.width <= 0 || frame.height <= 0 {
                return Err("invalid video dimensions".into());
            }
            let stream = *(*self.format).streams.add(self.stream as usize);
            // A phone's MOV/MP4 carries its display matrix as stream side
            // data; a frame may carry its own.
            let side = av_frame_get_side_data(self.frame, AV_FRAME_DATA_DISPLAYMATRIX);
            let matrix = if !side.is_null() && (*side).size >= 36 {
                (*side).data
            } else {
                let params = &*(*stream).codecpar;
                let side = av_packet_side_data_get(
                    params.coded_side_data,
                    params.nb_coded_side_data,
                    AV_PKT_DATA_DISPLAYMATRIX,
                );
                if !side.is_null() && (*side).size >= 36 {
                    (*side).data
                } else {
                    ptr::null_mut()
                }
            };
            let orientation = if matrix.is_null() {
                Orientation::default()
            } else {
                Orientation::from_display_matrix(&ptr::read_unaligned(matrix.cast::<[i32; 9]>()))
            };
            let (width, height) = fit(
                frame.width as u32,
                frame.height as u32,
                orientation.sideways(),
                self.max_width,
            );
            if i64::from(width) * i64::from(height) > MAX_PIXELS {
                return Err("video frame is too large".into());
            }
            // No larger than the decoded frame, so these fit a c_int.
            let (width, height) = (width as c_int, height as c_int);
            if (*self.scaled).width != width || (*self.scaled).height != height {
                av_frame_unref(self.scaled);
                (*self.scaled).format = AV_PIX_FMT_RGBA;
                (*self.scaled).width = width;
                (*self.scaled).height = height;
                check(av_frame_get_buffer(self.scaled, 32))?;
            }
            (*self.scaled).color_range = AVCOL_RANGE_JPEG;
            (*self.scaled).colorspace = AVCOL_SPC_RGB;
            (*self.scaled).color_primaries = frame.color_primaries;
            (*self.scaled).color_trc = frame.color_trc;
            // FFmpeg's frame API handles changing formats and colour metadata.
            check(sws_scale_frame(self.scaler, self.scaled, self.frame))?;
            let scaled = &*self.scaled;
            let rgba = orient(width as usize, height as usize, orientation, |y| {
                std::slice::from_raw_parts(
                    scaled.data[0].offset(y as isize * scaled.linesize[0] as isize),
                    width as usize * 4,
                )
            });
            let (out_width, out_height) = if orientation.sideways() {
                (height, width)
            } else {
                (width, height)
            };
            let base = (*stream).time_base;
            let rate = av_guess_frame_rate(self.format, stream, self.frame);
            Ok(Frame {
                width: out_width as u32,
                height: out_height as u32,
                rgba,
                timestamp: if frame.best_effort_timestamp == RUDEL_NOPTS {
                    None
                } else {
                    seconds(frame.best_effort_timestamp, base.num, base.den)
                },
                duration: frame_duration(
                    seconds(frame.duration, base.num, base.den),
                    (rate.num, rate.den),
                ),
            })
        }
    }
}

impl Drop for Video<'_> {
    fn drop(&mut self) {
        unsafe {
            sws_free_context(&mut self.scaler);
            av_frame_free(&mut self.scaled);
            av_frame_free(&mut self.frame);
            av_packet_free(&mut self.packet);
            avcodec_free_context(&mut self.codec);
            avformat_close_input(&mut self.format);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::{fixture, rstest};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        path::{Path, PathBuf},
        sync::atomic::{AtomicBool, Ordering},
        time::Instant,
    };

    /// A two-frame 25 fps 4:4:4 Y4M clip, `luma(frame, x)` bright per column.
    fn clip(width: usize, height: usize, luma: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        let mut data = format!("YUV4MPEG2 W{width} H{height} F25:1 Ip A1:1 C444\n").into_bytes();
        for frame in 0..2 {
            data.extend_from_slice(b"FRAME\n");
            for _ in 0..height {
                data.extend((0..width).map(|x| luma(frame, x)));
            }
            data.extend(vec![128; width * height * 2]);
        }
        data
    }

    /// Dark then bright, each frame flat.
    fn flat(width: usize, height: usize) -> Vec<u8> {
        clip(width, height, |frame, _| [32, 200][frame])
    }

    fn write(dir: &tempfile::TempDir, name: &str, data: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, data).unwrap();
        path
    }

    fn open(path: &Path, max_width: u32) -> Video<'static> {
        Video::open(path.to_str().unwrap(), "rudel-test", max_width, &|| true).unwrap()
    }

    /// The red channel of output pixel (x, y).
    fn red(frame: &Frame, x: u32, y: u32) -> u8 {
        frame.rgba[((y * frame.width + x) * 4) as usize]
    }

    /// Install `matrix` as the stream's display matrix, as a phone's MOV/MP4
    /// carries it, without making these tests depend on an encoder CLI.
    fn set_display_matrix(video: &mut Video, matrix: [i32; 9]) {
        unsafe {
            let stream = *(*video.format).streams.add(video.stream as usize);
            let params = &mut *(*stream).codecpar;
            let side = av_packet_side_data_new(
                &mut params.coded_side_data,
                &mut params.nb_coded_side_data,
                AV_PKT_DATA_DISPLAYMATRIX,
                36,
                0,
            );
            assert!(!side.is_null());
            ptr::copy_nonoverlapping(matrix.as_ptr(), (*side).data.cast(), 9);
        }
    }

    /// A 16×8 two-frame clip, kept on disk as long as this is.
    struct Clip {
        dir: tempfile::TempDir,
        path: PathBuf,
    }

    impl Clip {
        fn path(&self) -> &str {
            self.path.to_str().unwrap()
        }
    }

    #[fixture]
    fn small_clip() -> Clip {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "clip.y4m", &flat(16, 8));
        Clip { dir, path }
    }

    #[test]
    fn decodes_and_scales_to_the_width_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "wide-日本語.y4m", &flat(1600, 200));
        let mut video = open(&path, 1280);
        let first = video.next_frame().unwrap().unwrap();
        assert_eq!((first.width, first.height), (1280, 160));
        assert_eq!(first.rgba.len(), 1280 * 160 * 4);
        assert!(first.rgba.chunks(4).all(|p| p[3] == 255), "opaque");
        assert_eq!(first.timestamp, Some(0.0));
        assert_eq!(first.duration, Duration::from_millis(40));
        let second = video.next_frame().unwrap().unwrap();
        assert!(second.rgba[0] > first.rgba[0] + 100);
        assert_eq!(second.timestamp, Some(0.04));
    }

    #[rstest]
    fn a_small_video_is_not_enlarged(small_clip: Clip) {
        let frame = open(&small_clip.path, 1280).next_frame().unwrap().unwrap();
        assert_eq!((frame.width, frame.height), (16, 8));
        assert_eq!(frame.rgba.len(), 16 * 8 * 4);
    }

    #[test]
    fn drains_then_rewinds_to_the_same_first_frame() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "loop.y4m", &flat(32, 16));
        let mut video = open(&path, 1280);
        let first = video.next_frame().unwrap().unwrap();
        video.next_frame().unwrap().unwrap();
        assert!(video.next_frame().unwrap().is_none());
        // Drained stays drained until it is rewound.
        assert!(video.next_frame().unwrap().is_none());
        for _ in 0..2 {
            video.rewind().unwrap();
            let again = video.next_frame().unwrap().unwrap();
            assert_eq!(again.rgba, first.rgba);
            assert_eq!(again.timestamp, Some(0.0));
        }
        // Rewinding mid-stream also starts over.
        video.rewind().unwrap();
        assert_eq!(video.next_frame().unwrap().unwrap().timestamp, Some(0.0));
    }

    #[test]
    fn a_display_matrix_turns_the_picture_upright() {
        // Left half dark, right half bright.
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            "halves.y4m",
            &clip(64, 16, |_, x| if x < 32 { 32 } else { 200 }),
        );
        let mut video = open(&path, 1280);
        // 90° counter-clockwise: the left half ends up at the bottom.
        set_display_matrix(&mut video, [0, -65536, 0, 65536, 0, 0, 0, 0, 1 << 30]);
        let turned = video.next_frame().unwrap().unwrap();
        assert_eq!((turned.width, turned.height), (16, 64));
        assert_eq!(turned.rgba.len(), 16 * 64 * 4);
        assert!(red(&turned, 8, 0) > 150, "top is the bright right half");
        assert!(red(&turned, 8, 63) < 80, "bottom is the dark left half");
    }

    #[test]
    fn a_turned_video_is_fitted_by_its_shown_width() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "tall.y4m", &flat(400, 1600));
        let mut video = open(&path, 1280);
        set_display_matrix(&mut video, [0, 65536, 0, -65536, 0, 0, 0, 0, 1 << 30]);
        let frame = video.next_frame().unwrap().unwrap();
        assert_eq!((frame.width, frame.height), (1280, 320));
    }

    #[test]
    fn a_mirrored_display_matrix_mirrors_rather_than_turning() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            "halves.y4m",
            &clip(64, 16, |_, x| if x < 32 { 32 } else { 200 }),
        );
        let mut video = open(&path, 1280);
        set_display_matrix(&mut video, [-65536, 0, 0, 0, 65536, 0, 0, 0, 1 << 30]);
        let mirrored = video.next_frame().unwrap().unwrap();
        assert_eq!((mirrored.width, mirrored.height), (64, 16));
        assert!(red(&mirrored, 0, 8) > 150, "left is the bright right half");
        assert!(red(&mirrored, 63, 8) < 80, "right is the dark left half");
    }

    #[rstest]
    #[case::nul_in_the_path("bad\0path", "", 1280, "video path contains a NUL")]
    #[case::nul_in_the_user_agent("{path}", "bad\0agent", 1280, "user-agent contains a NUL")]
    #[case::zero_width("{path}", "", 0, "invalid video width limit")]
    #[case::over_the_width_limit("{path}", "", 16385, "invalid video width limit")]
    fn refuses_bad_arguments(
        small_clip: Clip,
        #[case] input: &str,
        #[case] agent: &str,
        #[case] width: u32,
        #[case] error: &str,
    ) {
        let input = input.replace("{path}", small_clip.path());
        assert_eq!(
            Video::open(&input, agent, width, &|| true).map(drop),
            Err(error.to_owned())
        );
    }

    #[rstest]
    fn opens_at_either_end_of_the_width_range(small_clip: Clip, #[values(1, 16384)] width: u32) {
        assert!(Video::open(small_clip.path(), "", width, &|| true).is_ok());
    }

    #[rstest]
    #[case::missing(None)]
    #[case::not_a_video(Some(&b"not a video at all"[..]))]
    fn reports_ffmpeg_errors_for_what_is_not_a_video(
        small_clip: Clip,
        #[case] contents: Option<&[u8]>,
    ) {
        let path = small_clip.dir.path().join("notes.mp4");
        if let Some(contents) = contents {
            std::fs::write(&path, contents).unwrap();
        }
        let error = Video::open(path.to_str().unwrap(), "", 1280, &|| true)
            .map(drop)
            .unwrap_err();
        assert!(error.starts_with("FFmpeg: "), "{error}");
    }

    #[rstest]
    #[case::concat("concat:{path}|{path}")]
    #[case::subfile("subfile,,start,0,end,0,:{path}")]
    #[case::pipe("pipe:0")]
    fn a_path_is_a_file_never_another_protocol(small_clip: Clip, #[case] input: &str) {
        let input = input.replace("{path}", small_clip.path());
        assert!(
            Video::open(&input, "", 1280, &|| true).is_err(),
            "{input} opened as a protocol"
        );
    }

    // Windows refuses a colon in a file name.
    #[cfg(unix)]
    #[test]
    fn a_colon_in_a_file_name_is_not_a_protocol() {
        // Only a relative name is at risk: a `/` before the colon already
        // stops FFmpeg reading a protocol out of it. Tests run in the package
        // directory; the guard removes the clip however the test ends.
        struct Remove(PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let name = Remove(PathBuf::from(format!("take:{}.y4m", std::process::id())));
        std::fs::write(&name.0, flat(16, 8)).unwrap();
        assert!(name.0.is_relative());
        assert_eq!(open(&name.0, 1280).next_frame().unwrap().unwrap().width, 16);
    }

    #[rstest]
    fn cancelling_stops_the_next_frame(small_clip: Clip) {
        let wanted = AtomicBool::new(true);
        let still_wanted = || wanted.load(Ordering::Relaxed);
        let mut video = Video::open(small_clip.path(), "", 1280, &still_wanted).unwrap();
        video.next_frame().unwrap().unwrap();
        wanted.store(false, Ordering::Relaxed);
        assert_eq!(video.next_frame().map(drop), Err("video cancelled".into()));
        // A panicking callback cancels rather than unwinding through C.
        let panics = || -> bool { panic!("callback bug") };
        assert!(Video::open(small_clip.path(), "", 1280, &panics).is_err());
    }

    #[test]
    fn streams_http_with_the_user_agent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/video.y4m", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            let len = socket.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..len]).contains("User-Agent: rudel-test"));
            let data = flat(16, 8);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                data.len()
            )
            .unwrap();
            socket.write_all(&data).unwrap();
        });
        let mut video = Video::open(&url, "rudel-test", 1280, &|| true).unwrap();
        assert_eq!(video.next_frame().unwrap().unwrap().width, 16);
        drop(video);
        server.join().unwrap();
    }

    #[test]
    fn interrupts_a_stalled_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/stalled", listener.local_addr().unwrap());
        let (release, wait) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (_socket, _) = listener.accept().unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(5));
        });
        let start = Instant::now();
        let wanted = || start.elapsed() < Duration::from_millis(250);
        assert!(Video::open(&url, "rudel-test", 1280, &wanted).is_err());
        let elapsed = start.elapsed();
        let _ = release.send(());
        server.join().unwrap();
        assert!(
            elapsed < Duration::from_secs(3),
            "I/O did not cancel: {elapsed:?}"
        );
    }
}
