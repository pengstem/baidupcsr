use baidupcsr_preview::{
    parse_image_metadata, ImagePreviewFormat, PreviewAction, PreviewKind, PreviewPlan,
};

#[test]
fn preview_plan_covers_images_pdf_archives_and_directories() {
    let directory = PreviewPlan::for_entry("photos", true, 0, 16 * 1024 * 1024);
    let image = PreviewPlan::for_entry("cover.png", false, 4096, 16 * 1024 * 1024);
    let pdf = PreviewPlan::for_entry("paper.pdf", false, 1024 * 1024, 16 * 1024 * 1024);
    let archive = PreviewPlan::for_entry("bundle.zip", false, 1024 * 1024, 16 * 1024 * 1024);

    assert_eq!(directory.kind, PreviewKind::Directory);
    assert_eq!(directory.action, PreviewAction::MetadataOnly);
    assert_eq!(image.kind, PreviewKind::Image);
    assert_eq!(image.action, PreviewAction::FetchImageMetadata);
    assert_eq!(pdf.kind, PreviewKind::Pdf);
    assert_eq!(pdf.action, PreviewAction::MetadataOnly);
    assert_eq!(archive.kind, PreviewKind::Archive);
    assert_eq!(archive.action, PreviewAction::MetadataOnly);
}

#[test]
fn png_header_preview_extracts_dimensions_without_full_file_decode() {
    let bytes = [
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0,
        0, 1, 0x40, 0, 0, 0, 0xf0, 8, 2, 0, 0, 0,
    ];

    let metadata = parse_image_metadata(&bytes);

    assert_eq!(metadata.format, ImagePreviewFormat::Png);
    assert_eq!(metadata.width, Some(320));
    assert_eq!(metadata.height, Some(240));
    assert!(!metadata.disk_backed);
}

#[test]
fn subtitle_files_are_text_preview_candidates() {
    let srt = PreviewPlan::for_entry("movie.zh.srt", false, 4096, 16 * 1024 * 1024);
    let ass = PreviewPlan::for_entry("movie.zh.ass", false, 4096, 16 * 1024 * 1024);

    assert_eq!(srt.kind, PreviewKind::Text);
    assert_eq!(srt.action, PreviewAction::FetchText);
    assert_eq!(ass.kind, PreviewKind::Text);
    assert_eq!(ass.action, PreviewAction::FetchText);
}
