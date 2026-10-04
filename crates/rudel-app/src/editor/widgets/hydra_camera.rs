//! hydra's `s0.initCam(index)`: a webcam as a live source (`hydra_live`).

use super::{hydra_images::Picture, hydra_live};
use eframe::egui;
use nokhwa::{
    Camera,
    pixel_format::RgbFormat,
    utils::{
        CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
    },
};
use std::sync::Arc;

/// The latest frame of camera `index`, starting it on first use.
pub(super) fn frame(ctx: &egui::Context, index: u32) -> Option<Arc<Picture>> {
    hydra_live::latest(ctx, &format!("initCam({index})"), move |sink| {
        capture(sink, index)
    })
}

fn capture(sink: &hydra_live::Sink, index: u32) -> Result<(), String> {
    // 720p at 30 fps where the camera offers it: what a background needs,
    // where asking for the highest rate can pick 1080p at 1 fps.
    let wanted = CameraFormat::new(Resolution::new(1280, 720), FrameFormat::MJPEG, 30);
    let mut camera = Camera::new(
        CameraIndex::Index(index),
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(wanted)),
    )
    .map_err(|e| e.to_string())?;
    camera.open_stream().map_err(|e| e.to_string())?;
    while sink.wanted() {
        let buffer = camera.frame().map_err(|e| e.to_string())?;
        let resolution = buffer.resolution();
        sink.publish(decode(
            buffer.source_frame_format(),
            resolution.width(),
            resolution.height(),
            buffer.buffer(),
        )?);
    }
    Ok(())
}

/// A frame as RGBA. Most webcams send MJPEG; YUYV is the usual
/// uncompressed fallback.
fn decode(format: FrameFormat, width: u32, height: u32, data: &[u8]) -> Result<Picture, String> {
    let rgba = match format {
        FrameFormat::MJPEG => image::load_from_memory(data)
            .map_err(|e| e.to_string())?
            .into_rgba8()
            .into_raw(),
        FrameFormat::YUYV => yuyv_to_rgba(data),
        FrameFormat::RAWRGB => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        other => {
            return Err(format!(
                "the camera sends {other:?}, which rudel cannot read"
            ));
        }
    };
    if rgba.len() != (width * height * 4) as usize {
        return Err(format!(
            "a {width}x{height} frame came out as {} bytes",
            rgba.len()
        ));
    }
    Ok(Picture {
        width,
        height,
        rgba,
    })
}

/// YUYV 4:2:2 (BT.601, studio range): each four bytes are two pixels sharing
/// their chroma.
fn yuyv_to_rgba(data: &[u8]) -> Vec<u8> {
    let pixel = |y: u8, u: u8, v: u8| {
        let c = 1.164 * (f32::from(y) - 16.0);
        let (d, e) = (f32::from(u) - 128.0, f32::from(v) - 128.0);
        let clamp = |x: f32| x.round().clamp(0.0, 255.0) as u8;
        [
            clamp(c + 1.596 * e),
            clamp(c - 0.392 * d - 0.813 * e),
            clamp(c + 2.017 * d),
            255,
        ]
    };
    data.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[y0, u, y1, v]| [pixel(y0, u, v), pixel(y1, u, v)])
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_reads_black_white_and_grey() {
        // Studio range: Y 16 is black, 235 white; neutral chroma is 128.
        let rgba = yuyv_to_rgba(&[16, 128, 235, 128]);
        assert_eq!(rgba, [0, 0, 0, 255, 255, 255, 255, 255]);
        let picture = decode(FrameFormat::YUYV, 2, 1, &[126, 128, 126, 128]).expect("decodes");
        assert!(
            picture.rgba[..3].iter().all(|&c| c.abs_diff(128) <= 1),
            "{:?}",
            picture.rgba
        );
        assert!(
            decode(FrameFormat::YUYV, 4, 1, &[16, 128, 235, 128]).is_err(),
            "short frame"
        );
    }
}
