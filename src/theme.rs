//! Design tokens and egui styling, generated from `brand/tokens.json`.
//! Values are kept in sync with the tokens file by hand.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Visuals};

pub const BLACK: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const LINE: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2c);
pub const MUTED: Color32 = Color32::from_rgb(0x8f, 0x8f, 0x8f);
pub const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
pub const BEAK: Color32 = Color32::from_rgb(0xdb, 0x77, 0x00);

/// Single reading column, from `size.contentMaxWidth`.
pub const CONTENT_MAX_WIDTH: f32 = 640.0;
/// Body text size, from `typography.body`.
pub const BODY_SIZE: f32 = 17.0;
/// Label size, from `typography.label`.
pub const LABEL_SIZE: f32 = 11.2;
/// Button size, from `typography.button`.
pub const BUTTON_SIZE: f32 = 12.8;

pub const CHAKRA_REGULAR: &[u8] = include_bytes!("../brand/fonts/ChakraPetch-Regular.ttf");
pub const CHAKRA_SEMIBOLD: &[u8] = include_bytes!("../brand/fonts/ChakraPetch-SemiBold.ttf");
pub const SHARE_TECH_MONO: &[u8] = include_bytes!("../brand/fonts/ShareTechMono-Regular.ttf");
pub const BARCODE: &[u8] = include_bytes!("../brand/fonts/LibreBarcode39Text-Regular.ttf");

pub const FONT_BODY: &str = "NB Body";
pub const FONT_DISPLAY: &str = "NB Display";
pub const FONT_MONO: &str = "NB Mono";
pub const FONT_BARCODE: &str = "NB Barcode";

pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        FONT_BODY.to_owned(),
        std::sync::Arc::new(FontData::from_static(CHAKRA_REGULAR)),
    );
    fonts.font_data.insert(
        FONT_DISPLAY.to_owned(),
        std::sync::Arc::new(FontData::from_static(CHAKRA_SEMIBOLD)),
    );
    fonts.font_data.insert(
        FONT_MONO.to_owned(),
        std::sync::Arc::new(FontData::from_static(SHARE_TECH_MONO)),
    );
    fonts.font_data.insert(
        FONT_BARCODE.to_owned(),
        std::sync::Arc::new(FontData::from_static(BARCODE)),
    );

    // Keep egui's bundled fonts behind ours, so characters that Chakra
    // Petch or Share Tech Mono lack (other scripts, symbols) still render
    // instead of falling through to a font-less run.
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let family = |first: &[&str]| {
        let mut names: Vec<String> = first.iter().map(|name| (*name).to_owned()).collect();
        for name in &fallback {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    };

    fonts
        .families
        .insert(FontFamily::Proportional, family(&[FONT_BODY, FONT_DISPLAY]));
    fonts
        .families
        .insert(FontFamily::Monospace, family(&[FONT_MONO, FONT_BODY]));
    fonts.families.insert(
        FontFamily::Name(FONT_DISPLAY.into()),
        family(&[FONT_DISPLAY, FONT_BODY]),
    );
    fonts.families.insert(
        FontFamily::Name(FONT_BODY.into()),
        family(&[FONT_BODY, FONT_DISPLAY]),
    );
    fonts.families.insert(
        FontFamily::Name(FONT_MONO.into()),
        family(&[FONT_MONO, FONT_BODY]),
    );
    fonts.families.insert(
        FontFamily::Name(FONT_BARCODE.into()),
        family(&[FONT_BARCODE, FONT_BODY]),
    );
    fonts
}

pub fn visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BLACK;
    v.window_fill = BLACK;
    v.extreme_bg_color = BLACK;
    v.faint_bg_color = BLACK;
    v.widgets.noninteractive.bg_fill = BLACK;
    v.widgets.inactive.bg_fill = BLACK;
    v.widgets.hovered.bg_fill = BLACK;
    v.widgets.active.bg_fill = BLACK;
    v.widgets.open.bg_fill = BLACK;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, LINE);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, LINE);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, MUTED);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, MUTED);
    v.selection.bg_fill = BEAK;
    v.selection.stroke = egui::Stroke::new(1.0, BEAK);
    v.hyperlink_color = TEXT;
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.popup_shadow = egui::Shadow::NONE;
    v.window_shadow = egui::Shadow::NONE;
    v
}

/// Upper-case, wide-tracked Share Tech Mono label, per `typography.label`.
pub fn label_font() -> FontId {
    FontId::new(LABEL_SIZE, FontFamily::Name(FONT_MONO.into()))
}

/// Button font, per `typography.button`.
pub fn button_font() -> FontId {
    FontId::new(BUTTON_SIZE, FontFamily::Name(FONT_MONO.into()))
}

/// Body font at the reader's chosen size.
pub fn body_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_BODY.into()))
}

/// Heading font (Chakra Petch 600) at the reader's chosen size.
pub fn heading_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_DISPLAY.into()))
}
