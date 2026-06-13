use gtk4::{gdk, glib, prelude::Cast};
use lumen_systray::types::item::IconPixmap;

const ICON_EXTENSIONS: [&str; 3] = ["png", "svg", "xpm"];

pub(super) fn select_best_pixmap(pixmaps: &[IconPixmap], target_size: i32) -> Option<&IconPixmap> {
    pixmaps.iter().min_by_key(|pixmap| {
        let large_enough = pixmap.width >= target_size && pixmap.height >= target_size;
        let size_delta = (pixmap.width - target_size).abs() + (pixmap.height - target_size).abs();

        // Prefer the smallest pixmap that is at least as large as the rendered
        // icon size. Falling back to a smaller pixmap is only desirable when the
        // tray item did not provide a sufficiently detailed image.
        (!large_enough, size_delta)
    })
}

pub(super) fn create_texture_from_pixmap(pixmap: &IconPixmap) -> Option<gdk::Texture> {
    let rgba_data = argb_to_rgba(&pixmap.data);
    let bytes = glib::Bytes::from_owned(rgba_data);

    gdk::MemoryTexture::new(
        pixmap.width,
        pixmap.height,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        (pixmap.width * 4) as usize,
    )
    .upcast::<gdk::Texture>()
    .into()
}

pub(super) fn load_icon_from_theme_path(theme_path: &str, icon_name: &str) -> Option<gdk::Texture> {
    if theme_path.is_empty() {
        return None;
    }

    for ext in ICON_EXTENSIONS {
        let file_path = format!("{theme_path}/{icon_name}.{ext}");
        if let Ok(texture) = gdk::Texture::from_filename(&file_path) {
            return Some(texture);
        }
    }

    None
}

fn argb_to_rgba(argb: &[u8]) -> Vec<u8> {
    argb.chunks_exact(4)
        .flat_map(|chunk| {
            let a = chunk[0];
            let r = chunk[1];
            let g = chunk[2];
            let b = chunk[3];
            [r, g, b, a]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argb_to_rgba_single_pixel() {
        let argb = [0xFF, 0x11, 0x22, 0x33];
        let rgba = argb_to_rgba(&argb);
        assert_eq!(rgba, vec![0x11, 0x22, 0x33, 0xFF]);
    }

    #[test]
    fn argb_to_rgba_multiple_pixels() {
        let argb = [0xFF, 0x11, 0x22, 0x33, 0x80, 0xAA, 0xBB, 0xCC];
        let rgba = argb_to_rgba(&argb);
        assert_eq!(rgba, vec![0x11, 0x22, 0x33, 0xFF, 0xAA, 0xBB, 0xCC, 0x80,]);
    }

    #[test]
    fn argb_to_rgba_empty() {
        let argb: [u8; 0] = [];
        let rgba = argb_to_rgba(&argb);
        assert!(rgba.is_empty());
    }

    #[test]
    fn argb_to_rgba_ignores_trailing_bytes() {
        let argb = [0xFF, 0x11, 0x22, 0x33, 0xAA, 0xBB];
        let rgba = argb_to_rgba(&argb);
        assert_eq!(rgba, vec![0x11, 0x22, 0x33, 0xFF]);
    }

    #[test]
    fn select_best_pixmap_exact_match() {
        let pixmaps = vec![
            IconPixmap {
                width: 16,
                height: 16,
                data: vec![],
            },
            IconPixmap {
                width: 24,
                height: 24,
                data: vec![],
            },
            IconPixmap {
                width: 48,
                height: 48,
                data: vec![],
            },
        ];
        let best = select_best_pixmap(&pixmaps, 24);
        assert!(best.is_some());
        let best = best.unwrap();
        assert_eq!(best.width, 24);
        assert_eq!(best.height, 24);
    }

    #[test]
    fn select_best_pixmap_prefers_larger_when_available() {
        let pixmaps = vec![
            IconPixmap {
                width: 16,
                height: 16,
                data: vec![],
            },
            IconPixmap {
                width: 28,
                height: 28,
                data: vec![],
            },
            IconPixmap {
                width: 64,
                height: 64,
                data: vec![],
            },
        ];
        let best = select_best_pixmap(&pixmaps, 24);
        assert!(best.is_some());
        let best = best.unwrap();
        assert_eq!(best.width, 28);
    }

    #[test]
    fn select_best_pixmap_closest_smaller() {
        let pixmaps = vec![
            IconPixmap {
                width: 8,
                height: 8,
                data: vec![],
            },
            IconPixmap {
                width: 20,
                height: 20,
                data: vec![],
            },
        ];
        let best = select_best_pixmap(&pixmaps, 24);
        assert!(best.is_some());
        let best = best.unwrap();
        assert_eq!(best.width, 20);
    }

    #[test]
    fn select_best_pixmap_uses_configured_target_size() {
        let pixmaps = vec![
            IconPixmap {
                width: 24,
                height: 24,
                data: vec![],
            },
            IconPixmap {
                width: 48,
                height: 48,
                data: vec![],
            },
        ];
        let best = select_best_pixmap(&pixmaps, 40);
        assert!(best.is_some());
        assert_eq!(best.unwrap().width, 48);
    }

    #[test]
    fn select_best_pixmap_empty() {
        let pixmaps: Vec<IconPixmap> = vec![];
        let best = select_best_pixmap(&pixmaps, 24);
        assert!(best.is_none());
    }

    #[test]
    fn select_best_pixmap_single() {
        let pixmaps = vec![IconPixmap {
            width: 128,
            height: 128,
            data: vec![],
        }];
        let best = select_best_pixmap(&pixmaps, 24);
        assert!(best.is_some());
        assert_eq!(best.unwrap().width, 128);
    }

    #[test]
    fn select_best_pixmap_non_square() {
        let pixmaps = vec![
            IconPixmap {
                width: 32,
                height: 16,
                data: vec![],
            },
            IconPixmap {
                width: 24,
                height: 24,
                data: vec![],
            },
            IconPixmap {
                width: 16,
                height: 32,
                data: vec![],
            },
        ];
        let best = select_best_pixmap(&pixmaps, 24);
        assert!(best.is_some());
        let best = best.unwrap();
        assert_eq!(best.width, 24);
        assert_eq!(best.height, 24);
    }
}
