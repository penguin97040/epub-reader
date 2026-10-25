#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;

use epub_reader::ui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("--dump") {
        return dump(args.get(1).map(PathBuf::from));
    }

    let pending_path = args.first().map(PathBuf::from).filter(|p| p.is_file());

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("epub-reader")
        .with_inner_size([1100.0, 760.0])
        .with_min_inner_size([520.0, 480.0])
        .with_resizable(true)
        .with_icon(ui::window_icon().unwrap_or_else(|| {
            std::sync::Arc::new(egui::IconData {
                rgba: vec![0; 4],
                width: 1,
                height: 1,
            })
        }));

    if cfg!(target_os = "macos") {
        // Keep the native traffic lights, but let our toolbar own the title area.
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false);
    } else {
        // One integrated window: the toolbar is the title bar.
        viewport = viewport.with_decorations(false);
    }

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "epub-reader",
        options,
        Box::new(|cc| Ok(Box::new(ui::ReaderApp::new(cc, pending_path)))),
    )
}

/// Print a plain-text summary of a book, for headless verification.
fn dump(path: Option<PathBuf>) -> eframe::Result {
    let Some(path) = path else {
        eprintln!("usage: epub-reader --dump <book>");
        std::process::exit(2);
    };
    match epub_reader::book::open(&path) {
        Ok(book) => {
            println!("title: {}", book.title);
            println!("author: {}", book.author.as_deref().unwrap_or("-"));
            println!("format: {}", book.format);
            println!("chapters: {}", book.chapters.len());
            println!("contents: {}", book.toc.len());
            println!("assets: {}", book.assets.len());
            for (i, chapter) in book.chapters.iter().enumerate().take(3) {
                let blocks = epub_reader::render::parse_blocks(&chapter.html);
                println!("chapter {i}: {} blocks", blocks.len());
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
