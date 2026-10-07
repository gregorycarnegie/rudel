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
use ffi::*;

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

impl<'a> Video<'a> {
    pub fn open(
        input: &str,
        user_agent: &str,
        max_width: u32,
        wanted: &'a (dyn Fn() -> bool + Sync),
    ) -> Result<Self, String> {
        if max_width == 0 || max_width > 16384 {
            return Err("invalid video width limit".into());
        }
        let input = CString::new(input).map_err(|_| "video path contains a NUL")?;
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
        unsafe {
            (*video.scaler).flags = RUDEL_BILINEAR as _;
            (*video.format).interrupt_callback = AVIOInterruptCB {
                callback: Some(interrupt),
                opaque: (&mut *video.interrupt as *mut &(dyn Fn() -> bool + Sync)).cast(),
            };
            let mut options = ptr::null_mut();
            if input.as_bytes().starts_with(b"http://") || input.as_bytes().starts_with(b"https://")
            {
                let set = av_dict_set(&mut options, c"user_agent".as_ptr(), agent.as_ptr(), 0);
                if set < 0 {
                    av_dict_free(&mut options);
                    check(set)?;
                }
                let set = av_dict_set(&mut options, c"tls_verify".as_ptr(), c"1".as_ptr(), 0);
                if set < 0 {
                    av_dict_free(&mut options);
                    check(set)?;
                }
                // Static OpenSSL's compiled-in certificate directory belongs
                // to the build machine; use the operating system's CA bundle.
                #[cfg(not(windows))]
                {
                    for ca in [
                        c"/etc/ssl/cert.pem",
                        c"/etc/ssl/certs/ca-certificates.crt",
                        c"/etc/pki/tls/certs/ca-bundle.crt",
                    ] {
                        if std::path::Path::new(ca.to_str().unwrap()).is_file() {
                            let set =
                                av_dict_set(&mut options, c"ca_file".as_ptr(), ca.as_ptr(), 0);
                            if set < 0 {
                                av_dict_free(&mut options);
                                check(set)?;
                            }
                            break;
                        }
                    }
                }
            }
            let opened =
                avformat_open_input(&mut video.format, input.as_ptr(), ptr::null(), &mut options);
            av_dict_free(&mut options);
            check(opened)?;
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
            (*video.codec).max_pixels = 64 * 1024 * 1024;
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
            let side = av_frame_get_side_data(self.frame, AV_FRAME_DATA_DISPLAYMATRIX);
            let matrix = if !side.is_null() && (*side).size >= 36 {
                (*side).data.cast::<i32>()
            } else {
                let params = &*(*stream).codecpar;
                let side = av_packet_side_data_get(
                    params.coded_side_data,
                    params.nb_coded_side_data,
                    AV_PKT_DATA_DISPLAYMATRIX,
                );
                if !side.is_null() && (*side).size >= 36 {
                    (*side).data.cast()
                } else {
                    ptr::null_mut()
                }
            };
            // ponytail: phone videos use quarter-turn display matrices;
            // arbitrary affine transforms would need libavfilter.
            let turns = if matrix.is_null() {
                0
            } else {
                ((-av_display_rotation_get(matrix) / 90.0).round() as i32).rem_euclid(4) as u32
            };
            let display_width = if turns % 2 == 0 {
                frame.width
            } else {
                frame.height
            };
            let scale = (self.max_width as f64 / display_width as f64).min(1.0);
            let width = (frame.width as f64 * scale).round().max(1.0) as c_int;
            let height = (frame.height as f64 * scale).round().max(1.0) as c_int;
            if i64::from(width) * i64::from(height) > 64 * 1024 * 1024 {
                return Err("video frame is too large".into());
            }
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
            let (out_width, out_height) = if turns % 2 == 0 {
                (width, height)
            } else {
                (height, width)
            };
            let mut rgba = vec![0; out_width as usize * out_height as usize * 4];
            for y in 0..height as usize {
                let row = std::slice::from_raw_parts(
                    (*self.scaled).data[0].offset(y as isize * (*self.scaled).linesize[0] as isize),
                    width as usize * 4,
                );
                if turns == 0 {
                    rgba[y * row.len()..(y + 1) * row.len()].copy_from_slice(row);
                } else {
                    for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
                        let (ox, oy) = match turns {
                            1 => (height as usize - 1 - y, x),
                            2 => (width as usize - 1 - x, height as usize - 1 - y),
                            _ => (y, width as usize - 1 - x),
                        };
                        let offset = (oy * out_width as usize + ox) * 4;
                        rgba[offset..offset + 4].copy_from_slice(pixel);
                    }
                }
            }
            let base = (*stream).time_base;
            let seconds = |ticks: i64| ticks as f64 * f64::from(base.num) / f64::from(base.den);
            let rate = av_guess_frame_rate(self.format, stream, self.frame);
            let duration = if frame.duration > 0 {
                seconds(frame.duration)
            } else if rate.num > 0 && rate.den > 0 {
                f64::from(rate.den) / f64::from(rate.num)
            } else {
                1.0 / 30.0
            };
            Ok(Frame {
                width: out_width as u32,
                height: out_height as u32,
                rgba,
                timestamp: (frame.best_effort_timestamp != RUDEL_NOPTS)
                    .then(|| seconds(frame.best_effort_timestamp)),
                duration: Duration::try_from_secs_f64(duration)
                    .unwrap_or(Duration::from_millis(33)),
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
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::Instant,
    };

    fn clip(width: usize, height: usize) -> Vec<u8> {
        let mut data = format!("YUV4MPEG2 W{width} H{height} F25:1 Ip A1:1 C444\n").into_bytes();
        for luma in [32, 200] {
            data.extend_from_slice(b"FRAME\n");
            data.extend(vec![luma; width * height]);
            data.extend(vec![128; width * height * 2]);
        }
        data
    }

    #[test]
    fn decodes_scales_drains_rewinds_and_rotates_without_an_executable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide-日本語.y4m");
        std::fs::write(&path, clip(1600, 200)).unwrap();
        let mut video = Video::open(path.to_str().unwrap(), "rudel-test", 1280, &|| true).unwrap();
        let first = video.next_frame().unwrap().unwrap();
        assert_eq!((first.width, first.height), (1280, 160));
        assert_eq!(first.rgba.len(), 1280 * 160 * 4);
        assert_eq!(first.rgba[3], 255);
        assert_eq!(first.timestamp, Some(0.0));
        assert_eq!(first.duration, Duration::from_millis(40));
        let second = video.next_frame().unwrap().unwrap();
        assert!(second.rgba[0] > first.rgba[0] + 100);
        assert_eq!(second.timestamp, Some(0.04));
        assert!(video.next_frame().unwrap().is_none());
        video.rewind().unwrap();
        assert_eq!(video.next_frame().unwrap().unwrap().rgba, first.rgba);
        video.rewind().unwrap();
        // A phone's MOV/MP4 display matrix is stream side data. Install the
        // same metadata without making this check depend on an encoder CLI.
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
            let matrix = [0i32, -65536, 0, 65536, 0, 0, 0, 0, 1 << 30];
            ptr::copy_nonoverlapping(matrix.as_ptr(), (*side).data.cast(), 9);
        }
        let rotated = video.next_frame().unwrap().unwrap();
        assert_eq!((rotated.width, rotated.height), (200, 1600));
        assert_eq!(rotated.rgba.len(), 200 * 1600 * 4);
        assert!(Video::open("bad\0path", "", 1280, &|| true).is_err());
        assert!(Video::open(path.to_str().unwrap(), "", 0, &|| true).is_err());
    }

    #[test]
    fn streams_http_and_interrupts_a_stalled_server() {
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
            let data = clip(16, 8);
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
