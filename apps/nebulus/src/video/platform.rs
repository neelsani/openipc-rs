use openipc_video::FrameDimensions;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows", test))]
use openipc_video::PixelFormat;

#[cfg(target_os = "macos")]
pub(crate) fn macos_rgba(frame: &openipc_video::MacOsVideoFrame) -> Result<Vec<u8>, String> {
    use openipc_video::DecodedSurface;

    frame
        .with_mapped_planes(|planes| {
            let borrowed = planes
                .iter()
                .map(|plane| Plane {
                    data: plane.data(),
                    stride: plane.stride(),
                })
                .collect::<Vec<_>>();
            convert_planes(frame.pixel_format(), frame.dimensions(), &borrowed)
        })
        .map_err(|error| error.to_string())?
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_rgba(
    frame: &openipc_video::LinuxVideoFrame,
    dimensions: FrameDimensions,
) -> Result<Vec<u8>, String> {
    use openipc_video::DecodedSurface;

    frame
        .with_mapped_planes(|planes| {
            let pitches = frame.plane_pitches();
            let borrowed = planes
                .iter()
                .zip(pitches)
                .map(|(data, stride)| Plane { data, stride })
                .collect::<Vec<_>>();
            convert_planes(frame.pixel_format(), dimensions, &borrowed)
        })
        .map_err(|error| error.to_string())?
}

#[cfg(target_os = "windows")]
pub(crate) fn windows_rgba(frame: &openipc_video::WindowsNv12Frame) -> Result<Vec<u8>, String> {
    let dimensions = frame.dimensions();
    convert_planes(
        PixelFormat::Nv12VideoRange,
        dimensions,
        &[
            Plane {
                data: frame.y_plane(),
                stride: frame.stride(),
            },
            Plane {
                data: frame.uv_plane(),
                stride: frame.stride(),
            },
        ],
    )
}

#[cfg(target_os = "android")]
pub(crate) fn android_rgba(
    frame: &openipc_video::AndroidVideoFrame,
    dimensions: FrameDimensions,
) -> Result<Vec<u8>, String> {
    frame
        .with_mapped_planes(|planes| {
            let [y, u, v, ..] = planes else {
                return Err("Android decoder output did not expose Y/U/V planes".to_owned());
            };
            yuv420_to_rgba(
                dimensions.width as usize,
                dimensions.height as usize,
                frame.crop_origin(),
                y,
                u,
                v,
            )
        })
        .map_err(|error| error.to_string())?
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows", test))]
pub(crate) struct Plane<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) stride: usize,
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows", test))]
fn convert_planes(
    format: PixelFormat,
    dimensions: FrameDimensions,
    planes: &[Plane<'_>],
) -> Result<Vec<u8>, String> {
    match format {
        PixelFormat::Nv12VideoRange | PixelFormat::Nv12FullRange => {
            let [y, uv, ..] = planes else {
                return Err("NV12 frame did not expose two planes".to_owned());
            };
            nv12_to_rgba(
                dimensions.width as usize,
                dimensions.height as usize,
                y,
                uv,
                format == PixelFormat::Nv12FullRange,
            )
        }
        PixelFormat::Bgra8 => {
            let [plane, ..] = planes else {
                return Err("BGRA frame did not expose a plane".to_owned());
            };
            bgra_to_rgba(dimensions.width as usize, dimensions.height as usize, plane)
        }
        _ => Err(format!("unsupported presentation pixel format {format:?}")),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows", test))]
fn bgra_to_rgba(width: usize, height: usize, plane: &Plane<'_>) -> Result<Vec<u8>, String> {
    let row_bytes = width
        .checked_mul(4)
        .ok_or_else(|| "BGRA row size overflowed".to_owned())?;
    let required = plane_len(height, plane.stride, row_bytes)
        .ok_or_else(|| "BGRA plane layout overflowed".to_owned())?;
    if plane.data.len() < required {
        return Err("BGRA plane is shorter than its layout".to_owned());
    }
    let output_len = row_bytes
        .checked_mul(height)
        .ok_or_else(|| "BGRA output size overflowed".to_owned())?;
    let mut rgba = vec![0; output_len];
    for row in 0..height {
        let source = &plane.data[row * plane.stride..row * plane.stride + row_bytes];
        let destination = &mut rgba[row * row_bytes..(row + 1) * row_bytes];
        for (bgra, rgba) in source.chunks_exact(4).zip(destination.chunks_exact_mut(4)) {
            rgba.copy_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
        }
    }
    Ok(rgba)
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows", test))]
fn nv12_to_rgba(
    width: usize,
    height: usize,
    y_plane: &Plane<'_>,
    uv_plane: &Plane<'_>,
    full_range: bool,
) -> Result<Vec<u8>, String> {
    let y_required = plane_len(height, y_plane.stride, width)
        .ok_or_else(|| "NV12 Y plane layout overflowed".to_owned())?;
    let uv_rows = height.div_ceil(2);
    let uv_row_bytes = width
        .div_ceil(2)
        .checked_mul(2)
        .ok_or_else(|| "NV12 UV row size overflowed".to_owned())?;
    let uv_required = plane_len(uv_rows, uv_plane.stride, uv_row_bytes)
        .ok_or_else(|| "NV12 UV plane layout overflowed".to_owned())?;
    if y_plane.data.len() < y_required {
        return Err("NV12 Y plane is shorter than its layout".to_owned());
    }
    if uv_plane.data.len() < uv_required {
        return Err("NV12 UV plane is shorter than its layout".to_owned());
    }
    let output_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "decoded frame dimensions overflowed".to_owned())?;
    let mut rgba = vec![0; output_len];
    for row in 0..height {
        let y_row = row.saturating_mul(y_plane.stride);
        let uv_row = (row / 2).saturating_mul(uv_plane.stride);
        for column in 0..width {
            let y = i32::from(y_plane.data[y_row + column]);
            let uv = uv_row + (column / 2) * 2;
            let u = i32::from(uv_plane.data[uv]) - 128;
            let v = i32::from(uv_plane.data[uv + 1]) - 128;
            let (c, scale) = if full_range {
                (y, 256)
            } else {
                ((y - 16).max(0), 298)
            };
            let red = (scale * c + 409 * v + 128) >> 8;
            let green = (scale * c - 100 * u - 208 * v + 128) >> 8;
            let blue = (scale * c + 516 * u + 128) >> 8;
            let offset = (row * width + column) * 4;
            rgba[offset] = red.clamp(0, 255) as u8;
            rgba[offset + 1] = green.clamp(0, 255) as u8;
            rgba[offset + 2] = blue.clamp(0, 255) as u8;
            rgba[offset + 3] = 255;
        }
    }
    Ok(rgba)
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows", test))]
fn plane_len(rows: usize, stride: usize, row_bytes: usize) -> Option<usize> {
    if rows == 0 {
        return Some(0);
    }
    rows.checked_sub(1)?
        .checked_mul(stride)?
        .checked_add(row_bytes)
}

#[cfg(target_os = "android")]
fn yuv420_to_rgba(
    width: usize,
    height: usize,
    crop: [usize; 2],
    y_plane: &openipc_video::AndroidImagePlane<'_>,
    u_plane: &openipc_video::AndroidImagePlane<'_>,
    v_plane: &openipc_video::AndroidImagePlane<'_>,
) -> Result<Vec<u8>, String> {
    let output_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "decoded frame dimensions overflowed".to_owned())?;
    let mut rgba = vec![0; output_len];
    for row in 0..height {
        let source_y = crop[1] + row;
        let chroma_y = source_y / 2;
        for column in 0..width {
            let source_x = crop[0] + column;
            let chroma_x = source_x / 2;
            let y_index = source_y
                .saturating_mul(y_plane.row_stride())
                .saturating_add(source_x.saturating_mul(y_plane.pixel_stride()));
            let u_index = chroma_y
                .saturating_mul(u_plane.row_stride())
                .saturating_add(chroma_x.saturating_mul(u_plane.pixel_stride()));
            let v_index = chroma_y
                .saturating_mul(v_plane.row_stride())
                .saturating_add(chroma_x.saturating_mul(v_plane.pixel_stride()));
            let y = i32::from(
                *y_plane
                    .data()
                    .get(y_index)
                    .ok_or_else(|| "Android Y plane is shorter than its layout".to_owned())?,
            );
            let u = i32::from(
                *u_plane
                    .data()
                    .get(u_index)
                    .ok_or_else(|| "Android U plane is shorter than its layout".to_owned())?,
            ) - 128;
            let v = i32::from(
                *v_plane
                    .data()
                    .get(v_index)
                    .ok_or_else(|| "Android V plane is shorter than its layout".to_owned())?,
            ) - 128;
            write_video_range_rgba(&mut rgba, row * width + column, y, u, v);
        }
    }
    Ok(rgba)
}

#[cfg(target_os = "android")]
fn write_video_range_rgba(rgba: &mut [u8], pixel: usize, y: i32, u: i32, v: i32) {
    let c = (y - 16).max(0);
    let red = (298 * c + 409 * v + 128) >> 8;
    let green = (298 * c - 100 * u - 208 * v + 128) >> 8;
    let blue = (298 * c + 516 * u + 128) >> 8;
    let offset = pixel * 4;
    rgba[offset] = red.clamp(0, 255) as u8;
    rgba[offset + 1] = green.clamp(0, 255) as u8;
    rgba[offset + 2] = blue.clamp(0, 255) as u8;
    rgba[offset + 3] = 255;
}

#[cfg(test)]
mod tests {
    use super::{bgra_to_rgba, nv12_to_rgba, Plane};

    #[test]
    fn bgra_is_swizzled_to_rgba_without_copying_row_padding() {
        let data = [3, 2, 1, 4, 30, 20, 10, 40, 0, 0, 0, 0];
        let rgba = bgra_to_rgba(
            2,
            1,
            &Plane {
                data: &data,
                stride: 12,
            },
        )
        .unwrap();
        assert_eq!(rgba, [1, 2, 3, 4, 10, 20, 30, 40]);
    }

    #[test]
    fn neutral_nv12_converts_to_gray_rgba() {
        let y = [126; 4];
        let uv = [128, 128];
        let rgba = nv12_to_rgba(
            2,
            2,
            &Plane {
                data: &y,
                stride: 2,
            },
            &Plane {
                data: &uv,
                stride: 2,
            },
            false,
        )
        .unwrap();
        assert_eq!(rgba.len(), 16);
        assert!(rgba
            .chunks_exact(4)
            .all(|pixel| { pixel[0] == pixel[1] && pixel[1] == pixel[2] && pixel[3] == 255 }));
    }

    #[test]
    fn short_nv12_plane_is_rejected() {
        let error = nv12_to_rgba(
            4,
            2,
            &Plane {
                data: &[16; 4],
                stride: 4,
            },
            &Plane {
                data: &[128; 4],
                stride: 4,
            },
            false,
        )
        .unwrap_err();
        assert!(error.contains("Y plane"));
    }
}
