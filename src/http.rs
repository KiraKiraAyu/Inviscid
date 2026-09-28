pub mod cache;
pub mod client;
pub mod dimensions;
pub mod path;

pub use cache::*;
pub use client::*;
pub use dimensions::*;
pub use path::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_cache_lru_eviction() {
        let mut cache = ImageCache::new();

        for i in 0..1024 {
            let url = format!("https://example.com/image_{}.png", i);
            cache.mark_loaded(&url);
            cache.record_size(&url, 100, 200);
        }
        assert_eq!(cache.len(), 1024);
        assert!(cache.is_loaded("https://example.com/image_0.png"));

        // Touch image_0 so image_1 becomes the least recently used entry
        assert_eq!(
            cache.get_size("https://example.com/image_0.png"),
            Some((100, 200))
        );

        cache.mark_loaded("https://example.com/image_1024.png");
        assert_eq!(cache.len(), 1024);

        assert!(cache.is_loaded("https://example.com/image_0.png"));
        assert!(!cache.is_loaded("https://example.com/image_1.png"));
        assert!(cache.is_loaded("https://example.com/image_1024.png"));
    }

    #[test]
    fn test_image_cache_unified_metadata() {
        let mut cache = ImageCache::new();
        let url = "https://example.com/pic.jpg";

        cache.record_size(url, 800, 600);
        assert_eq!(cache.get_size(url), Some((800, 600)));
        assert!(!cache.is_loaded(url));
        assert!(!cache.is_failed(url));

        cache.mark_loaded(url);
        assert!(cache.is_loaded(url));
        assert!(!cache.is_failed(url));
        assert_eq!(cache.get_size(url), Some((800, 600)));

        cache.mark_failed(url);
        assert!(!cache.is_loaded(url));
        assert!(cache.is_failed(url));
    }

    #[test]
    fn test_extract_png_dimensions() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0, 0, 0, 13]); // IHDR chunk length
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);

        assert_eq!(extract_image_dimensions(&png), Some((1920, 1080)));
    }

    #[test]
    fn test_extract_gif_dimensions() {
        // GIF87a 320 x 240
        let mut gif87 = b"GIF87a".to_vec();
        gif87.extend_from_slice(&320u16.to_le_bytes());
        gif87.extend_from_slice(&240u16.to_le_bytes());
        assert_eq!(extract_image_dimensions(&gif87), Some((320, 240)));

        // GIF89a 800 x 600
        let mut gif89 = b"GIF89a".to_vec();
        gif89.extend_from_slice(&800u16.to_le_bytes());
        gif89.extend_from_slice(&600u16.to_le_bytes());
        assert_eq!(extract_image_dimensions(&gif89), Some((800, 600)));
    }

    #[test]
    fn test_extract_webp_dimensions() {
        // WebP VP8 (Lossy) 640 x 480
        let mut vp8 = b"RIFF".to_vec();
        vp8.extend_from_slice(&100u32.to_le_bytes()); // file size
        vp8.extend_from_slice(b"WEBPVP8 ");
        vp8.extend_from_slice(&40u32.to_le_bytes()); // chunk size
        vp8.extend_from_slice(&[0, 0, 0]); // frame tag
        vp8.extend_from_slice(&[0x9D, 0x01, 0x2A]); // sync code
        vp8.extend_from_slice(&640u16.to_le_bytes());
        vp8.extend_from_slice(&480u16.to_le_bytes());
        assert_eq!(extract_image_dimensions(&vp8), Some((640, 480)));

        // WebP VP8L (Lossless) 100 x 200
        let mut vp8l = b"RIFF".to_vec();
        vp8l.extend_from_slice(&100u32.to_le_bytes());
        vp8l.extend_from_slice(b"WEBPVP8L");
        vp8l.extend_from_slice(&40u32.to_le_bytes());
        vp8l.push(0x2F); // signature
        let w_minus_1 = 99u32;
        let h_minus_1 = 199u32;
        let b0 = (w_minus_1 & 0xFF) as u8;
        let b1 = (((w_minus_1 >> 8) & 0x3F) | ((h_minus_1 & 0x03) << 6)) as u8;
        let b2 = ((h_minus_1 >> 2) & 0xFF) as u8;
        let b3 = ((h_minus_1 >> 10) & 0x0F) as u8;
        vp8l.extend_from_slice(&[b0, b1, b2, b3]);
        assert_eq!(extract_image_dimensions(&vp8l), Some((100, 200)));

        // WebP VP8X (Extended) 1920 x 1080
        let mut vp8x = b"RIFF".to_vec();
        vp8x.extend_from_slice(&100u32.to_le_bytes());
        vp8x.extend_from_slice(b"WEBPVP8X");
        vp8x.extend_from_slice(&10u32.to_le_bytes()); // chunk size
        vp8x.extend_from_slice(&[0, 0, 0, 0]); // flags
        let w24 = 1920u32 - 1;
        let h24 = 1080u32 - 1;
        vp8x.extend_from_slice(&[
            (w24 & 0xFF) as u8,
            ((w24 >> 8) & 0xFF) as u8,
            ((w24 >> 16) & 0xFF) as u8,
        ]);
        vp8x.extend_from_slice(&[
            (h24 & 0xFF) as u8,
            ((h24 >> 8) & 0xFF) as u8,
            ((h24 >> 16) & 0xFF) as u8,
        ]);
        assert_eq!(extract_image_dimensions(&vp8x), Some((1920, 1080)));
    }

    #[test]
    fn test_extract_jpeg_dimensions() {
        // Construct JPEG with SOI, padding 0xFF, APP1 segment, and SOF0 marker
        let mut jpeg = vec![0xFF, 0xD8]; // SOI
        // APP0 marker (len 16)
        jpeg.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
        jpeg.extend_from_slice(&[0u8; 14]);
        // 0xFF padding before SOF0
        jpeg.extend_from_slice(&[0xFF, 0xFF, 0xC0]); // SOF0
        jpeg.extend_from_slice(&[0x00, 0x11]); // length = 17
        jpeg.push(8); // precision
        jpeg.extend_from_slice(&720u16.to_be_bytes()); // height = 720
        jpeg.extend_from_slice(&1280u16.to_be_bytes()); // width = 1280
        jpeg.extend_from_slice(&[0u8; 10]); // components

        assert_eq!(extract_image_dimensions(&jpeg), Some((1280, 720)));
    }

    #[test]
    fn test_extract_bmp_dimensions() {
        // Positive height (bottom-up)
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0u8; 16]); // padding to offset 18
        bmp.extend_from_slice(&500i32.to_le_bytes()); // width = 500
        bmp.extend_from_slice(&300i32.to_le_bytes()); // height = 300
        assert_eq!(extract_image_dimensions(&bmp), Some((500, 300)));

        // Negative height (top-down)
        let mut bmp_top_down = b"BM".to_vec();
        bmp_top_down.extend_from_slice(&[0u8; 16]);
        bmp_top_down.extend_from_slice(&500i32.to_le_bytes());
        bmp_top_down.extend_from_slice(&(-300i32).to_le_bytes());
        assert_eq!(extract_image_dimensions(&bmp_top_down), Some((500, 300)));
    }

    #[test]
    fn test_extract_corrupted_or_truncated() {
        assert_eq!(extract_image_dimensions(&[]), None);
        assert_eq!(extract_image_dimensions(&[0x89, b'P', b'N']), None);
        assert_eq!(extract_image_dimensions(&[0xFF, 0xD8, 0xFF, 0xC0]), None);
    }

    #[test]
    fn test_percent_decoding_and_path_resolution() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("photo%2B1%2Epng"), "photo+1.png");
        assert_eq!(percent_decode("%E4%BD%A0%E5%A5%BD"), "你好");
        assert_eq!(percent_decode("plain_text.png"), "plain_text.png");

        let doc_dir = std::env::temp_dir();
        let doc_path = doc_dir.join("test.md");

        let resolved = parse_file_url_path("images/demo.png", Some(&doc_path)).unwrap();
        assert_eq!(resolved, doc_dir.join("images/demo.png"));

        #[cfg(target_os = "windows")]
        {
            let file_url = "file:///C:/My%20Folder/test%20image.png";
            let parsed = parse_file_url_path(file_url, None).unwrap();
            assert_eq!(
                parsed,
                std::path::PathBuf::from("C:/My Folder/test image.png")
            );
        }

        assert!(parse_file_url_path("https://example.com/img.png", Some(&doc_path)).is_none());
    }

    #[test]
    fn test_ureq_client_http_client_compliance() {
        let client = UreqClient::new();
        assert_eq!(client.type_name(), "UreqClient");
        assert!(client.user_agent().is_some());
        let _ = client.proxy();
    }

    #[test]
    fn test_http_worker_pool_concurrency() {
        let pool = HttpWorkerPool::new(4);
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel();

        for _ in 0..8 {
            let c = counter.clone();
            let tx_clone = tx.clone();
            pool.spawn(move || {
                c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = tx_clone.send(());
            });
        }

        for _ in 0..8 {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        }
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 8);
    }

    #[test]
    fn test_image_cache_versioning_and_notification() {
        let initial_v = image_cache_version();
        let mut sub = subscribe_image_updates();

        // Record size increments version and notifies
        record_image_size("https://example.com/stream_test.png", 640, 480);
        let v1 = image_cache_version();
        assert!(v1 > initial_v);
        assert!(sub.try_recv().is_ok());

        // Mark loaded increments version and notifies
        mark_url_loaded("https://example.com/stream_test.png");
        let v2 = image_cache_version();
        assert!(v2 > v1);
        assert!(sub.try_recv().is_ok());

        // Mark failed increments version and notifies
        mark_url_failed("https://example.com/failed_test.png");
        let v3 = image_cache_version();
        assert!(v3 > v2);
        assert!(sub.try_recv().is_ok());
    }
}
