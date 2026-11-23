//! AItelier editor: node graph, parameters, 3D viewport and timeline in one window.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console window in release builds

mod app;
mod edit;
mod library;
mod node_editor;
mod params;
mod timeline;
mod viewport;

/// The window icon (same picture as the .exe icon).
fn icon() -> Option<egui::IconData> {
    let decoder = png::Decoder::new(std::io::Cursor::new(
        include_bytes!("../assets/icon.png").as_slice(),
    ));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba: buf,
        width: info.width,
        height: info.height,
    })
}

fn main() -> eframe::Result {
    let path = std::env::args().nth(1).map(std::path::PathBuf::from);
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1500.0, 900.0])
        .with_min_inner_size([900.0, 560.0])
        .with_title("AItelier");
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "AItelier",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, path)))),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_decodes() {
        let i = super::icon().expect("icon decodes");
        assert_eq!((i.width, i.height), (256, 256));
        assert_eq!(i.rgba.len(), 256 * 256 * 4);
    }
}
