use anyhow::{Result, ensure};
use std::{path::Path, time::Duration};

// Diagnostic readback only, once per case; never enabled in performance runs.
pub fn save(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    composite: &crate::composite::Composite,
    config: &wgpu::SurfaceConfiguration,
    path: &Path,
) -> Result<()> {
    ensure!(
        matches!(
            config.format,
            wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
                | wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
        ),
        "capture needs an 8-bit surface format"
    );
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("p0.capture"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let stride = (config.width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("p0.readback"),
        size: stride as u64 * config.height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    composite.draw(&mut encoder, &texture.create_view(&Default::default()));
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(10)),
    })?;
    rx.recv_timeout(Duration::from_secs(10))??;
    let pixels = buffer.slice(..).get_mapped_range()?;
    let mut rgba = Vec::with_capacity((config.width * config.height * 4) as usize);
    for row in pixels.chunks_exact(stride as usize) {
        for p in row[..config.width as usize * 4].chunks_exact(4) {
            let mut color = [p[0], p[1], p[2], p[3]];
            if matches!(
                config.format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            ) {
                color.swap(0, 2);
            }
            // PNG stores straight alpha. The tested Metal path is PostMultiplied.
            if config.alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied && color[3] > 0 {
                for c in &mut color[..3] {
                    *c = ((*c as f32 * 255.) / p[3] as f32).min(255.) as u8;
                }
            }
            rgba.extend_from_slice(&color);
        }
    }
    image::save_buffer(
        path,
        &rgba,
        config.width,
        config.height,
        image::ColorType::Rgba8,
    )?;
    Ok(())
}

pub fn gallery(directory: &Path) -> Result<()> {
    let mut paths: Vec<_> = std::fs::read_dir(directory)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|e| e == "png")
                && p.file_stem()
                    .is_some_and(|n| n.to_string_lossy().starts_with("case-"))
        })
        .collect();
    paths.sort();
    ensure!(
        !paths.is_empty(),
        "no case images in {}",
        directory.display()
    );
    let (w, h) = (180, 216);
    let mut gallery = image::RgbaImage::new(w * 2 * 4, h * (paths.len() as u32).div_ceil(4));
    for (i, path) in paths.iter().enumerate() {
        let thumbnail = image::open(path)?
            .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let ox = (i as u32 % 4) * w * 2;
        let oy = (i as u32 / 4) * h;
        for background in 0..2 {
            for y in 0..h {
                for x in 0..w {
                    let p = thumbnail.get_pixel(x, y).0;
                    let a = p[3] as u32;
                    let bg = if background == 0 { 245 } else { 28 };
                    gallery.put_pixel(
                        ox + background * w + x,
                        oy + y,
                        image::Rgba([
                            ((p[0] as u32 * a + bg * (255 - a)) / 255) as u8,
                            ((p[1] as u32 * a + bg * (255 - a)) / 255) as u8,
                            ((p[2] as u32 * a + bg * (255 - a)) / 255) as u8,
                            255,
                        ]),
                    );
                }
            }
        }
    }
    gallery.save(directory.join("gallery.png"))?;
    std::fs::write(
        directory.join("gallery-index.json"),
        serde_json::to_vec_pretty(
            &paths
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>(),
        )?,
    )?;
    Ok(())
}
