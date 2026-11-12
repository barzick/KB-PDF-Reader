//! Making files smaller and cropping pages.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageBuffer, Luma, Rgb};
use lopdf::{Document, Object, SaveOptions};
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{pdfium, KbError, Result};

fn err(e: impl std::fmt::Display) -> KbError {
    KbError::Pdf(e.to_string())
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CompressLevel {
    /// Lossless only: streams, unused objects, object streams.
    Lossless,
    Light,
    Medium,
    Strong,
}

impl CompressLevel {
    /// Longest image side in pixels, and JPEG quality.
    fn image(self) -> Option<(u32, u8)> {
        match self {
            Self::Lossless => None,
            Self::Light => Some((2600, 85)),
            Self::Medium => Some((1800, 72)),
            Self::Strong => Some((1200, 55)),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CompressResult {
    pub before: usize,
    pub after: usize,
    pub images: usize,
}

fn name(o: Option<&Object>) -> Option<&[u8]> {
    o.and_then(|o| o.as_name().ok())
}

/// Colour components of an image colour space we can re-encode (1 or 3).
fn components(doc: &Document, cs: Option<&Object>) -> Option<u8> {
    let cs = match cs? {
        Object::Reference(r) => doc.get_object(*r).ok()?,
        o => o,
    };
    match cs {
        Object::Name(n) if n == b"DeviceRGB" => Some(3),
        Object::Name(n) if n == b"DeviceGray" => Some(1),
        Object::Array(a) if name(a.first()) == Some(b"ICCBased") => {
            let s = match a.get(1)? {
                Object::Reference(r) => doc.get_object(*r).ok()?.as_stream().ok()?,
                _ => return None,
            };
            match s.dict.get(b"N").ok()?.as_i64().ok()? {
                1 => Some(1),
                3 => Some(3),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Re-encodes one image stream if that makes it smaller. Returns true when changed.
fn shrink_image(doc: &mut Document, id: lopdf::ObjectId, max_side: u32, quality: u8) -> bool {
    let Ok(stream) = doc.get_object(id).and_then(|o| o.as_stream()) else { return false };
    let d = &stream.dict;
    if name(d.get(b"Subtype").ok()) != Some(b"Image")
        || d.get(b"ImageMask").ok().and_then(|o| o.as_bool().ok()) == Some(true)
        || d.has(b"Mask")
        || d.has(b"Decode")
        || d.get(b"BitsPerComponent").ok().and_then(|o| o.as_i64().ok()) != Some(8)
    {
        return false;
    }
    let (Some(w), Some(h)) = (
        d.get(b"Width").ok().and_then(|o| o.as_i64().ok()),
        d.get(b"Height").ok().and_then(|o| o.as_i64().ok()),
    ) else {
        return false;
    };
    let (w, h) = (w as u32, h as u32);
    if w < 16 || h < 16 {
        return false;
    }
    let Some(n) = components(doc, d.get(b"ColorSpace").ok()) else { return false };
    let filters: Vec<Vec<u8>> = match d.get(b"Filter") {
        Ok(Object::Name(f)) => vec![f.clone()],
        Ok(Object::Array(a)) => a.iter().filter_map(|o| o.as_name().ok().map(|n| n.to_vec())).collect(),
        _ => vec![],
    };
    let old_len = stream.content.len();
    let img: DynamicImage = if filters == [b"DCTDecode".to_vec()] {
        match image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg) {
            Ok(i) => i,
            Err(_) => return false,
        }
    } else if filters.iter().all(|f| f == b"FlateDecode") {
        let Ok(raw) = stream.decompressed_content() else { return false };
        if raw.len() < (w * h * n as u32) as usize {
            return false;
        }
        let raw = raw[..(w * h * n as u32) as usize].to_vec();
        if n == 1 {
            match ImageBuffer::<Luma<u8>, _>::from_raw(w, h, raw) {
                Some(b) => DynamicImage::ImageLuma8(b),
                None => return false,
            }
        } else {
            match ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, raw) {
                Some(b) => DynamicImage::ImageRgb8(b),
                None => return false,
            }
        }
    } else {
        return false;
    };
    let img = if w.max(h) > max_side {
        img.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let img = if n == 1 { DynamicImage::ImageLuma8(img.to_luma8()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
    let mut jpeg = vec![];
    if JpegEncoder::new_with_quality(Cursor::new(&mut jpeg), quality).encode_image(&img).is_err() {
        return false;
    }
    // Only worth it when clearly smaller.
    if jpeg.len() as f32 > old_len as f32 * 0.9 {
        return false;
    }
    let Ok(stream) = doc.get_object_mut(id).and_then(|o| o.as_stream_mut()) else { return false };
    stream.dict.set("Width", img.width() as i64);
    stream.dict.set("Height", img.height() as i64);
    stream.dict.set("Filter", "DCTDecode");
    stream.dict.remove(b"DecodeParms");
    stream.set_content(jpeg);
    stream.allows_compression = false;
    true
}

/// Smaller file: re-encoded images (except lossless), compressed streams,
/// unused objects removed, objects packed in object streams.
pub fn compress(bytes: &[u8], level: CompressLevel) -> Result<(Vec<u8>, CompressResult)> {
    let mut doc = Document::load_mem(bytes).map_err(err)?;
    if doc.is_encrypted() {
        return Err(err("document protégé par mot de passe"));
    }
    let mut images = 0;
    if let Some((side, q)) = level.image() {
        let ids: Vec<_> = doc.objects.keys().copied().collect();
        for id in ids {
            if shrink_image(&mut doc, id, side, q) {
                images += 1;
            }
        }
    }
    doc.prune_objects();
    doc.delete_zero_length_streams();
    doc.compress();
    let mut out = vec![];
    let opts = SaveOptions::builder().use_object_streams(true).use_xref_streams(true).compression_level(9).build();
    doc.save_with_options(&mut out, opts).map_err(err)?;
    // Keep the original when nothing was gained.
    if out.len() >= bytes.len() {
        out = bytes.to_vec();
    }
    let r = CompressResult { before: bytes.len(), after: out.len(), images };
    Ok((out, r))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Crop {
    /// Margins to cut, in points, as the page is displayed: [top, right, bottom, left].
    Margins { margins: [f32; 4] },
    /// Around what is drawn on the page, plus a margin in points.
    Content { pad: f32 },
    /// Back to the full page.
    Reset,
}

/// Sets the visible area (CropBox) of pages.
pub fn crop(bytes: &[u8], pages: Option<&[u32]>, how: &Crop) -> Result<Vec<u8>> {
    // Content bounds come from PDFium.
    let content: Vec<Option<[f32; 4]>> = if let Crop::Content { .. } = how {
        let doc = pdfium()?.load_pdf_from_byte_slice(bytes, None)?;
        let mut v = vec![];
        for page in doc.pages().iter() {
            let (pw, ph) = (page.width().value, page.height().value);
            let mut r: Option<[f32; 4]> = None;
            for obj in page.objects().iter() {
                let Ok(b) = obj.bounds() else { continue };
                let (l, bt, rt, t) = (b.left().value, b.bottom().value, b.right().value, b.top().value);
                // A background covering the page says nothing about the content.
                if (rt - l) * (t - bt) > pw * ph * 0.9 {
                    continue;
                }
                r = Some(match r {
                    None => [l, bt, rt, t],
                    Some(a) => [a[0].min(l), a[1].min(bt), a[2].max(rt), a[3].max(t)],
                });
            }
            // Form fields, notes and other annotations count as content too.
            for a in page.annotations().iter() {
                if let Ok(b) = a.bounds() {
                    let (l, bt, rt, t) = (b.left().value, b.bottom().value, b.right().value, b.top().value);
                    if rt - l > 0.5 && t - bt > 0.5 {
                        r = Some(match r {
                            None => [l, bt, rt, t],
                            Some(a) => [a[0].min(l), a[1].min(bt), a[2].max(rt), a[3].max(t)],
                        });
                    }
                }
            }
            v.push(r);
        }
        v
    } else {
        vec![]
    };

    let mut doc = Document::load_mem(bytes).map_err(err)?;
    if doc.is_encrypted() {
        return Err(err("document protégé par mot de passe"));
    }
    let all = doc.get_pages();
    for (n, id) in all.iter().map(|(n, id)| (*n - 1, *id)) {
        if pages.is_some_and(|p| !p.contains(&n)) {
            continue;
        }
        // MediaBox (possibly inherited) and rotation.
        let mut media = None;
        let mut rotate = 0i64;
        let mut d = doc.get_dictionary(id).map_err(err)?;
        for _ in 0..32 {
            if media.is_none() {
                if let Ok(o) = d.get(b"MediaBox") {
                    let o = match o {
                        Object::Reference(r) => doc.get_object(*r).map_err(err)?,
                        o => o,
                    };
                    let v: Vec<f32> = o.as_array().map_err(err)?.iter().filter_map(|x| x.as_float().ok()).collect();
                    if v.len() == 4 {
                        media = Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])]);
                    }
                }
            }
            if rotate == 0 {
                rotate = d.get(b"Rotate").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
            }
            match d.get(b"Parent").ok().and_then(|o| o.as_reference().ok()).and_then(|p| doc.get_dictionary(p).ok()) {
                Some(p) => d = p,
                None => break,
            }
        }
        let m = media.unwrap_or([0.0, 0.0, 595.28, 841.89]);
        let rect = match how {
            Crop::Reset => None,
            Crop::Margins { margins: [t, r, b, l] } => {
                // Displayed sides → user-space sides [left, bottom, right, top].
                let [ul, ub, ur, ut] = match rotate.rem_euclid(360) {
                    90 => [*t, *l, *b, *r],
                    180 => [*r, *t, *l, *b],
                    270 => [*b, *r, *t, *l],
                    _ => [*l, *b, *r, *t],
                };
                Some([m[0] + ul, m[1] + ub, m[2] - ur, m[3] - ut])
            }
            Crop::Content { pad } => content.get(n as usize).copied().flatten().map(|c| {
                [(c[0] - pad).max(m[0]), (c[1] - pad).max(m[1]), (c[2] + pad).min(m[2]), (c[3] + pad).min(m[3])]
            }),
        };
        let page = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()).map_err(err)?;
        match rect {
            Some(r) if r[2] - r[0] >= 18.0 && r[3] - r[1] >= 18.0 => {
                page.set("CropBox", Object::Array(r.iter().map(|&v| Object::Real(v)).collect()));
            }
            Some(_) => return Err(err(format!("page {} : la zone restante serait trop petite", n + 1))),
            None => {
                page.remove(b"CropBox");
            }
        }
    }
    let mut out = vec![];
    doc.save_to(&mut out).map_err(err)?;
    Ok(out)
}
