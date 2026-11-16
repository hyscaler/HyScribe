// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::hooks::NoHooks;
use crate::tests::corpus;

/// Quality filters: `wp_editor_set_quality` gives the width, `jpeg_quality`
/// adds ten; each records what it was asked.
struct Quality(std::sync::Mutex<Vec<String>>);

impl Hooks for Quality {
    fn wp_editor_set_quality(&self, quality: i64, mime: &str, size: (u32, u32)) -> i64 {
        self.0
            .lock()
            .unwrap()
            .push(format!("{quality} {mime} {size:?}"));
        i64::from(size.0)
    }

    fn jpeg_quality(&self, quality: i64, context: &str) -> i64 {
        self.0
            .lock()
            .unwrap()
            .push(format!("jpeg {quality} {context}"));
        quality + 10
    }

    fn image_make_intermediate_size(&self, file: String) -> String {
        format!("moved/{file}.x")
    }
}

#[test]
fn sets_the_quality_through_the_filters() {
    let hooks = Quality(Default::default());
    let photo = corpus("photo.jpg");
    let mut editor = Editor::load(&photo, "2026/11/photo.jpg", &Limits::default(), &hooks).unwrap();
    // 640 + 10, out of range: the default.
    assert_eq!(editor.quality, 82);
    assert!(editor.resize(90, 0, &Crop::No));
    assert_eq!(editor.quality, 100);
    assert!(editor.resize(0, 1, &Crop::No));
    // 1 + 10 is 11; a filter's 0 would be 1.
    assert_eq!(editor.quality, 11);
    assert_eq!(
        *hooks.0.lock().unwrap(),
        [
            "82 image/jpeg (640, 480)",
            "jpeg 640 image_resize",
            "82 image/jpeg (90, 68)",
            "jpeg 90 image_resize",
            "82 image/jpeg (1, 1)",
            "jpeg 1 image_resize"
        ]
    );
    let saved = editor.save(None).unwrap();
    assert_eq!(
        (saved.path.as_str(), saved.file.as_str()),
        ("2026/11/photo-1x1.jpg", "photo-1x1.jpg.x")
    );
    struct Zero;
    impl Hooks for Zero {
        fn wp_editor_set_quality(&self, _: i64, _: &str, _: (u32, u32)) -> i64 {
            0
        }
    }
    let png = corpus("rgb.png");
    assert_eq!(
        Editor::load(&png, "rgb.png", &Limits::default(), &Zero)
            .unwrap()
            .quality,
        1
    );
}

#[test]
fn samples_first_only_below_a_ninth_of_the_area() {
    let photo = corpus("small.jpg");
    let mut editor = Editor::load(&photo, "small.jpg", &Limits::default(), &NoHooks).unwrap();
    editor.mime = "image/png";
    let data = (0..1000 * 1000u64)
        .map(|i| (i * 7919 % 251) as u8)
        .collect();
    let image = Pixels {
        width: 1000,
        height: 1000,
        channels: 1,
        data,
    };
    // Exactly 0.111 of the area: filtered directly.
    editor.image = image.clone();
    editor.crop((0, 0, 1000, 1000), (111, 1000));
    assert_eq!(editor.image, image.resize(111, 1000));
    // Sampled only where five times the target is over 128 a side.
    editor.image = image.clone();
    editor.crop((0, 0, 1000, 1000), (25, 400));
    assert_eq!(editor.image, image.resize(25, 400));
    editor.image = image.clone();
    editor.crop((0, 0, 1000, 1000), (400, 25));
    assert_eq!(editor.image, image.resize(400, 25));
    editor.image = image.clone();
    editor.crop((0, 0, 1000, 1000), (26, 26));
    assert_eq!(editor.image, image.sample(130, 130).resize(26, 26));
    // Just under: point sampled to five times the size first.
    editor.image = image.clone();
    editor.crop((0, 0, 1000, 1000), (110, 1000));
    assert_eq!(editor.image, image.sample(550, 5000).resize(110, 1000));
}

#[test]
fn names_files_as_generate_filename_does() {
    let photo = corpus("photo.jpg");
    let editor =
        Editor::load(&photo, "2026/11/My.Photo.JPG", &Limits::default(), &NoHooks).unwrap();
    assert_eq!(
        editor.generate_filename(None),
        "2026/11/My.Photo-640x480.jpg"
    );
    assert_eq!(
        editor.generate_filename(Some("scaled")),
        "2026/11/My.Photo-scaled.jpg"
    );
    assert_eq!(editor.generate_filename(Some("")), "2026/11/My.Photo.jpg");
    let bare = Editor::load(&photo, "photo", &Limits::default(), &NoHooks).unwrap();
    assert_eq!(bare.generate_filename(None), "photo-640x480.");
    assert_eq!(bare.save(Some("photo".into())).unwrap().file, "photo");
    assert!(Editor::load(b"no image", "x.jpg", &Limits::default(), &NoHooks).is_err());
    // What its format cannot hold is not saved.
    let mut wide = Editor::load(&photo, "photo.jpg", &Limits::default(), &NoHooks).unwrap();
    wide.image = Pixels {
        width: 70_000,
        height: 1,
        channels: 3,
        data: vec![0; 210_000],
    };
    assert_eq!(wide.save(None), Err(Error::Unwritable));
}

#[test]
fn edits_as_the_imagick_editor_does() {
    let photo = corpus("photo.jpg");
    let mut editor = Editor::load(&photo, "photo.jpg", &Limits::default(), &NoHooks).unwrap();
    // Its own size is no change; no size comes of a larger one.
    assert!(editor.resize(640, 480, &Crop::No));
    assert!(!editor.resize(700, 500, &Crop::No));
    let same = SubSize {
        name: "same".into(),
        width: 640,
        height: 480,
        crop: Crop::No,
    };
    assert_eq!(editor.make_subsize(&same), None);
    let large = SubSize {
        name: "large".into(),
        width: 700,
        height: 700,
        crop: Crop::No,
    };
    assert_eq!(editor.make_subsize(&large), None);
    assert_eq!(editor.size(), (640, 480));
    // Turns and mirrors keep or swap the sides, and only for a JPEG.
    for (orientation, size, turned) in [
        (None, (640, 480), false),
        (Some(1), (640, 480), false),
        (Some(2), (640, 480), true),
        (Some(3), (640, 480), true),
        (Some(4), (640, 480), true),
        (Some(5), (480, 640), true),
        (Some(6), (480, 640), true),
        (Some(7), (480, 640), true),
        (Some(8), (480, 640), true),
        (Some(9), (640, 480), false),
    ] {
        let mut editor = Editor::load(&photo, "photo.jpg", &Limits::default(), &NoHooks).unwrap();
        assert_eq!(
            editor.maybe_exif_rotate(orientation),
            turned,
            "{orientation:?}"
        );
        assert_eq!(editor.size(), size, "{orientation:?}");
    }
    let png = corpus("rgb.png");
    let mut editor = Editor::load(&png, "rgb.png", &Limits::default(), &NoHooks).unwrap();
    assert!(!editor.maybe_exif_rotate(Some(6)));
    editor.rotate(90);
    assert_eq!(editor.size(), (300, 400));
    editor.rotate(360);
    assert_eq!(editor.size(), (300, 400));
    // A crop to no size keeps the crop's.
    editor.crop((10, 20, 30, 40), (0, 0));
    assert_eq!(editor.size(), (30, 40));
    editor.crop((0, 0, 30, 40), (15, 0));
    assert_eq!(editor.size(), (15, 40));
    editor.crop((0, 0, 15, 40), (0, 20));
    assert_eq!(editor.size(), (15, 20));
}
