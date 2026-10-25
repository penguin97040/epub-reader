//! Screens and widgets, styled with the project visual system: dark only,
//! solid black, square corners, one beak accent.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{
    Align, Align2, ColorImage, Context, CornerRadius, FontFamily, FontId, Frame, Id, Image, Key,
    Layout, Margin, Order, PointerButton, Pos2, Rangef, Rect, Response, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2, ViewportCommand,
};

use crate::book::{Book, TocNode};
use crate::render::{Align as HtmlAlign, Block, Inline, parse_blocks};
use crate::state::{Position, State};
use crate::theme;

const WINDOW_ICON: &[u8] = include_bytes!("../assets/icon-256.png");
const BAR_HEIGHT: f32 = 48.0;
const TOC_WIDTH: f32 = 190.0;

pub struct ReaderApp {
    pub state: State,
    screen: Screen,
    error: Option<String>,
    pending_path: Option<PathBuf>,
    textures: HashMap<String, TextureHandle>,
    blocks: HashMap<usize, Vec<Block>>,
    icon: Option<TextureHandle>,
    settings_open: bool,
}

enum Screen {
    Home,
    Reader(Box<ReaderState>),
    About,
}

struct ReaderState {
    book: Book,
    chapter: usize,
    show_toc: bool,
    fraction: f32,
    restore: Option<f32>,
    pending_offset: Option<f32>,
}

#[derive(Clone, Copy, PartialEq)]
enum BarAction {
    Home,
    Open,
    Contents,
    Bigger,
    Smaller,
    ToggleSettings,
}

struct BarOutput {
    actions: Vec<BarAction>,
    /// The settings button, so a click there can toggle instead of counting as "outside".
    settings_rect: Rect,
}

enum LinkAction {
    External(String),
    Internal(String),
}

impl ReaderApp {
    pub fn new(cc: &eframe::CreationContext<'_>, pending_path: Option<PathBuf>) -> Self {
        cc.egui_ctx.set_fonts(theme::font_definitions());
        cc.egui_ctx.set_visuals(theme::visuals());
        // We own the zoom shortcuts so the chosen UI scale is saved with the
        // rest of the settings.
        cc.egui_ctx
            .options_mut(|options| options.zoom_with_keyboard = false);

        let state = State::load();
        cc.egui_ctx.set_zoom_factor(state.ui_scale);
        let icon = window_icon().map(|data| {
            let image = ColorImage::from_rgba_unmultiplied(
                [data.width as usize, data.height as usize],
                data.rgba.as_slice(),
            );
            cc.egui_ctx
                .load_texture("app-icon", image, TextureOptions::LINEAR)
        });

        Self {
            state,
            screen: Screen::Home,
            error: None,
            pending_path,
            textures: HashMap::new(),
            blocks: HashMap::new(),
            icon,
            settings_open: false,
        }
    }

    fn open_book(&mut self, path: PathBuf, ctx: &Context) {
        match crate::book::open(&path) {
            Ok(book) => {
                let key = path.to_string_lossy().to_string();
                let saved = self.state.positions.get(&key).copied();
                self.state.note_recent(&key, &book.title);
                self.state.save();
                self.textures.clear();
                self.blocks.clear();
                self.error = None;
                let chapter = saved
                    .map(|p| p.chapter.min(book.chapters.len().saturating_sub(1)))
                    .unwrap_or(0);
                let fraction = saved.map(|p| p.fraction).unwrap_or(0.0);
                self.screen = Screen::Reader(Box::new(ReaderState {
                    chapter,
                    fraction,
                    restore: Some(fraction),
                    book,
                    show_toc: false,
                    pending_offset: None,
                }));
                ctx.request_repaint();
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.screen = Screen::Home;
            }
        }
    }

    fn save_position(&mut self) {
        if let Screen::Reader(r) = &self.screen {
            let key = r.book.path.to_string_lossy().to_string();
            self.state.positions.insert(
                key,
                Position {
                    chapter: r.chapter,
                    fraction: r.fraction,
                },
            );
        }
    }

    fn go_home(&mut self) {
        self.save_position();
        self.state.save();
        self.screen = Screen::Home;
    }

    fn pick_file(&mut self, ctx: &Context) {
        let picked = rfd::FileDialog::new()
            .set_title("Open a book")
            .add_filter("Books", &["epub", "kepub", "mobi", "azw", "prc"])
            .pick_file();
        if let Some(path) = picked {
            self.open_book(path, ctx);
        }
    }

    fn texture_for(&mut self, ctx: &Context, key: &str) -> Option<TextureHandle> {
        if let Some(handle) = self.textures.get(key) {
            return Some(handle.clone());
        }
        let bytes: Vec<u8> = if let Some(rest) = key.strip_prefix("data:") {
            let (_, b64) = rest.split_once(',')?;
            use base64::{Engine, engine::general_purpose::STANDARD};
            STANDARD.decode(b64.trim()).ok()?
        } else {
            self.current_asset(key)?
        };
        let mut img = image::load_from_memory(&bytes).ok()?;
        if img.width() > 1600 {
            let height = (img.height() as f32 * 1600.0 / img.width() as f32) as u32;
            img = img.resize_exact(1600, height.max(1), image::imageops::FilterType::Triangle);
        }
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = ColorImage::from_rgba_unmultiplied(size, rgba.as_flat_samples().as_slice());
        let handle = ctx.load_texture(format!("book-{key}"), color, TextureOptions::LINEAR);
        self.textures.insert(key.to_owned(), handle.clone());
        Some(handle)
    }

    fn current_asset(&self, key: &str) -> Option<Vec<u8>> {
        let Screen::Reader(reader) = &self.screen else {
            return None;
        };
        reader.book.assets.get(key).cloned().or_else(|| {
            let needle = key.rsplit('/').next().unwrap_or(key);
            reader
                .book
                .assets
                .iter()
                .find(|(k, _)| k.rsplit('/').next() == Some(needle))
                .map(|(_, v)| v.clone())
        })
    }
}

impl eframe::App for ReaderApp {
    fn ui(&mut self, root: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();

        self.handle_zoom_keys(&ctx);
        self.handle_window_resize(&ctx);
        self.handle_shortcuts(&ctx);
        self.take_dropped_files(&ctx);
        if let Some(path) = self.pending_path.take() {
            self.open_book(path, &ctx);
        }

        let bar = self.top_bar(root);
        self.apply_bar_actions(bar.actions, &ctx);

        match &mut self.screen {
            Screen::Home => self.home_ui(root),
            Screen::About => self.about_ui(root),
            Screen::Reader(_) => self.reader_ui(root),
        }

        self.settings_ui(&ctx, bar.settings_rect);
        frame.set_traffic_lights_position(&ctx, Rangef::new(0.0, BAR_HEIGHT), 12.0);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_position();
        self.state.save();
    }
}

impl ReaderApp {
    fn handle_zoom_keys(&mut self, ctx: &Context) {
        let (bigger, smaller, reset) = ctx.input(|i| {
            let command = i.modifiers.command;
            (
                command && (i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals)),
                command && i.key_pressed(Key::Minus),
                command && i.key_pressed(Key::Num0),
            )
        });

        let next = if reset {
            Some(1.0)
        } else if bigger {
            Some(self.state.ui_scale + 0.1)
        } else if smaller {
            Some(self.state.ui_scale - 0.1)
        } else {
            None
        };

        if let Some(scale) = next {
            self.set_ui_scale(ctx, scale);
        }
    }

    fn set_ui_scale(&mut self, ctx: &Context, scale: f32) {
        let scale = (scale * 10.0).round() / 10.0;
        self.state.ui_scale = scale.clamp(0.8, 2.0);
        ctx.set_zoom_factor(self.state.ui_scale);
        self.state.save();
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let (open, contents, settings) = ctx.input(|i| {
            let command = i.modifiers.command;
            (
                command && i.key_pressed(Key::O),
                command && i.key_pressed(Key::B),
                command && i.key_pressed(Key::Comma),
            )
        });

        if open {
            self.pick_file(ctx);
        }
        if contents && let Screen::Reader(reader) = &mut self.screen {
            reader.show_toc = !reader.show_toc;
        }
        if settings {
            self.settings_open = !self.settings_open;
        }
    }

    /// Draw a thin resize grip on the window edges, since there is no native
    /// frame to provide one on Windows and Linux.
    fn handle_window_resize(&self, ctx: &Context) {
        if cfg!(target_os = "macos") {
            return;
        }
        let maximized = ctx.input(|i| {
            i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
        });
        if maximized {
            return;
        }

        let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else {
            return;
        };
        let rect = ctx.content_rect();
        let margin = 5.0;
        let left = pos.x <= rect.left() + margin;
        let right = pos.x >= rect.right() - margin;
        let top = pos.y <= rect.top() + margin;
        let bottom = pos.y >= rect.bottom() - margin;

        let (direction, cursor) = match (left, right, top, bottom) {
            (true, _, true, _) => (
                Some(egui::ResizeDirection::NorthWest),
                egui::CursorIcon::ResizeNwSe,
            ),
            (_, true, true, _) => (
                Some(egui::ResizeDirection::NorthEast),
                egui::CursorIcon::ResizeNeSw,
            ),
            (true, _, _, true) => (
                Some(egui::ResizeDirection::SouthWest),
                egui::CursorIcon::ResizeNeSw,
            ),
            (_, true, _, true) => (
                Some(egui::ResizeDirection::SouthEast),
                egui::CursorIcon::ResizeNwSe,
            ),
            (true, _, _, _) => (
                Some(egui::ResizeDirection::West),
                egui::CursorIcon::ResizeHorizontal,
            ),
            (_, true, _, _) => (
                Some(egui::ResizeDirection::East),
                egui::CursorIcon::ResizeHorizontal,
            ),
            (_, _, true, _) => (
                Some(egui::ResizeDirection::North),
                egui::CursorIcon::ResizeVertical,
            ),
            (_, _, _, true) => (
                Some(egui::ResizeDirection::South),
                egui::CursorIcon::ResizeVertical,
            ),
            _ => (None, egui::CursorIcon::Default),
        };

        if let Some(direction) = direction {
            ctx.set_cursor_icon(cursor);
            if ctx.input(|i| i.pointer.primary_pressed()) {
                ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
            }
        }
    }

    fn apply_bar_actions(&mut self, actions: Vec<BarAction>, ctx: &Context) {
        for action in actions {
            match action {
                BarAction::Home => {
                    self.go_home();
                    ctx.request_repaint();
                }
                BarAction::Open => self.pick_file(ctx),
                BarAction::Contents => {
                    if let Screen::Reader(reader) = &mut self.screen {
                        reader.show_toc = !reader.show_toc;
                    }
                }
                BarAction::Bigger => {
                    self.state.font_size = (self.state.font_size + 1.0).min(40.0);
                    self.state.save();
                }
                BarAction::Smaller => {
                    self.state.font_size = (self.state.font_size - 1.0).max(12.0);
                    self.state.save();
                }
                BarAction::ToggleSettings => {
                    self.settings_open = !self.settings_open;
                    ctx.request_repaint();
                }
            }
        }
    }

    fn top_bar(&self, ui: &mut Ui) -> BarOutput {
        let is_reader = matches!(self.screen, Screen::Reader(_));
        let is_about = matches!(self.screen, Screen::About);
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        let settings_open = self.settings_open;
        let narrow = ui.available_width() < 820.0;
        let title = match &self.screen {
            Screen::Reader(reader) => match &reader.book.author {
                Some(author) => format!("{} — {}", reader.book.title, author),
                None => reader.book.title.clone(),
            },
            _ => String::new(),
        };
        let contents_open = match &self.screen {
            Screen::Reader(reader) => reader.show_toc,
            _ => false,
        };
        let icon = self.icon.clone();

        let mut actions = Vec::new();
        let mut settings_rect = Rect::NOTHING;

        egui::Panel::top("app_bar")
            .exact_size(BAR_HEIGHT)
            .frame(Frame::NONE.fill(theme::BLACK))
            .show(ui, |ui| {
                let rect = ui.max_rect();
                let drag = ui.interact(
                    rect,
                    ui.scope_id().with("app_bar_drag"),
                    Sense::click_and_drag(),
                );
                if drag.drag_started_by(PointerButton::Primary) {
                    ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
                }
                if drag.double_clicked() {
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                }

                ui.allocate_ui_with_layout(
                    rect.size(),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add_space(if cfg!(target_os = "macos") { 78.0 } else { 8.0 });
                        if let Some(icon) = &icon {
                            ui.add(Image::new(icon).fit_to_exact_size(Vec2::splat(22.0)));
                        }

                        if is_reader {
                            ui.add_space(4.0);
                            if icon_button(ui, Icon::Contents, contents_open)
                                .on_hover_text("Contents (Ctrl+B)")
                                .clicked()
                            {
                                actions.push(BarAction::Contents);
                            }
                        }

                        if is_reader || is_about {
                            ui.add_space(4.0);
                            if icon_button(ui, Icon::Back, false)
                                .on_hover_text("Library")
                                .clicked()
                            {
                                actions.push(BarAction::Home);
                            }
                            ui.add_space(6.0);
                            if is_about {
                                ui.label(muted_label("About & licences"));
                            } else if !narrow {
                                let title_width = (ui.available_width() - 330.0).clamp(60.0, 360.0);
                                ui.add_sized(
                                    [title_width, 20.0],
                                    egui::Label::new(
                                        RichText::new(&title)
                                            .font(theme::body_font(15.0))
                                            .color(theme::MUTED),
                                    )
                                    .truncate()
                                    .selectable(false),
                                );
                            }
                        } else {
                            ui.add_space(8.0);
                            if !narrow {
                                ui.label(
                                    RichText::new("EPUB READER")
                                        .font(FontId::new(
                                            15.0,
                                            FontFamily::Name(theme::FONT_DISPLAY.into()),
                                        ))
                                        .color(theme::TEXT),
                                );
                            }
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !cfg!(target_os = "macos") {
                                ui.add_space(4.0);
                                if icon_button(ui, Icon::Close, false)
                                    .on_hover_text("Close")
                                    .clicked()
                                {
                                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                                }
                                let (icon, tooltip) = if maximized {
                                    (Icon::Restore, "Restore")
                                } else {
                                    (Icon::Maximise, "Maximise")
                                };
                                if icon_button(ui, icon, false)
                                    .on_hover_text(tooltip)
                                    .clicked()
                                {
                                    ui.ctx()
                                        .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                                }
                                if icon_button(ui, Icon::Minimise, false)
                                    .on_hover_text("Minimise")
                                    .clicked()
                                {
                                    ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
                                }
                                ui.add_space(6.0);
                                bar_separator(ui);
                            }

                            let gear = icon_button(ui, Icon::Settings, settings_open)
                                .on_hover_text("Settings (Ctrl+,)");
                            settings_rect = gear.rect;
                            if gear.clicked() {
                                actions.push(BarAction::ToggleSettings);
                            }

                            ui.add_space(2.0);
                            if icon_button(ui, Icon::Open, false)
                                .on_hover_text("Open book (Ctrl+O)")
                                .clicked()
                            {
                                actions.push(BarAction::Open);
                            }

                            if is_reader {
                                ui.add_space(2.0);
                                if !narrow {
                                    if bar_text_button(ui, "A+")
                                        .on_hover_text("Bigger text")
                                        .clicked()
                                    {
                                        actions.push(BarAction::Bigger);
                                    }
                                    if bar_text_button(ui, "A−")
                                        .on_hover_text("Smaller text")
                                        .clicked()
                                    {
                                        actions.push(BarAction::Smaller);
                                    }
                                    ui.add_space(2.0);
                                }
                            }
                        });
                    },
                );

                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    Stroke::new(1.0, theme::LINE),
                );
            });

        BarOutput {
            actions,
            settings_rect,
        }
    }

    fn settings_ui(&mut self, ctx: &Context, gear_rect: Rect) {
        if !self.settings_open {
            return;
        }

        let width = (ctx.content_rect().width() - 24.0).clamp(260.0, 300.0);
        let x = (ctx.content_rect().right() - width - 32.0).max(10.0);
        let mut open_about = false;
        let mut reset = false;

        let popover = egui::Window::new("Settings")
            .id(Id::unique("settings_popover"))
            .order(Order::Tooltip)
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .movable(false)
            .fixed_pos(Pos2::new(x, BAR_HEIGHT + 8.0))
            .frame(
                Frame::NONE
                    .fill(theme::BLACK)
                    .stroke(Stroke::new(1.0, theme::LINE))
                    .inner_margin(Margin::symmetric(14, 10)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(width);
                ui.set_max_width(width);

                setting_row(ui, "Font size", |ui| {
                    let (minus, plus) = stepper(
                        ui,
                        &format!("{}", self.state.font_size.round()),
                        self.state.font_size > 12.0,
                        self.state.font_size < 40.0,
                    );
                    if minus {
                        self.state.font_size = (self.state.font_size - 1.0).max(12.0);
                        self.state.save();
                    }
                    if plus {
                        self.state.font_size = (self.state.font_size + 1.0).min(40.0);
                        self.state.save();
                    }
                });

                setting_row(ui, "Line spacing", |ui| {
                    let (minus, plus) = stepper(
                        ui,
                        &format!("{:.2}", self.state.line_height),
                        self.state.line_height > 1.2,
                        self.state.line_height < 2.2,
                    );
                    if minus {
                        self.state.line_height =
                            ((self.state.line_height - 0.05) * 100.0).round() / 100.0;
                        self.state.line_height = self.state.line_height.max(1.2);
                        self.state.save();
                    }
                    if plus {
                        self.state.line_height =
                            ((self.state.line_height + 0.05) * 100.0).round() / 100.0;
                        self.state.line_height = self.state.line_height.min(2.2);
                        self.state.save();
                    }
                });

                setting_row(ui, "UI scale", |ui| {
                    let (minus, plus) = stepper(
                        ui,
                        &format!("{:.0}%", self.state.ui_scale * 100.0),
                        self.state.ui_scale > 0.8,
                        self.state.ui_scale < 2.0,
                    );
                    if minus {
                        self.set_ui_scale(ctx, self.state.ui_scale - 0.1);
                    }
                    if plus {
                        self.set_ui_scale(ctx, self.state.ui_scale + 0.1);
                    }
                });

                setting_row(ui, "Column width", |ui| {
                    let (minus, plus) = stepper(
                        ui,
                        &format!("{:.0}", self.state.column_width),
                        self.state.column_width > 480.0,
                        self.state.column_width < 900.0,
                    );
                    if minus {
                        self.state.column_width = (self.state.column_width - 20.0).max(480.0);
                        self.state.save();
                    }
                    if plus {
                        self.state.column_width = (self.state.column_width + 20.0).min(900.0);
                        self.state.save();
                    }
                });

                ui.add_space(8.0);
                rule(ui);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(muted_label(&format!(
                        "Version {}",
                        env!("CARGO_PKG_VERSION")
                    )));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if link_button(ui, "Reset").clicked() {
                            reset = true;
                        }
                    });
                });

                ui.add_space(10.0);
                nz_mark(ui);
                ui.add_space(10.0);
                if link_button(ui, "About & licences").clicked() {
                    open_about = true;
                }
            });

        let pointer = ctx.input(|i| i.pointer.interact_pos());
        let pressed = ctx.input(|i| i.pointer.any_pressed());
        if pressed
            && let Some(pos) = pointer
            && let Some(response) = &popover
            && !response.response.rect.contains(pos)
            && !gear_rect.contains(pos)
        {
            self.settings_open = false;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.settings_open = false;
        }

        if reset {
            self.state.reset_reading();
            self.set_ui_scale(ctx, self.state.ui_scale);
            self.state.save();
        }
        if open_about {
            self.settings_open = false;
            self.save_position();
            self.screen = Screen::About;
            ctx.request_repaint();
        }
    }

    fn take_dropped_files(&mut self, ctx: &Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped.into_iter().next().map(|f| f.path().to_path_buf()) {
            self.open_book(path, ctx);
        }
    }

    // -- Home -------------------------------------------------------------

    fn home_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut open_request = false;
        let mut open_path: Option<PathBuf> = None;
        let mut forget: Option<String> = None;

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(theme::BLACK))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("home")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let top_space = if self.state.recents.is_empty() {
                            ((ui.available_height() - 260.0) * 0.5).clamp(24.0, 260.0)
                        } else {
                            48.0
                        };
                        ui.vertical_centered(|ui| {
                            ui.add_space(top_space);
                            barcode(ui, "*EPUB READER*", 44.0);
                            ui.add_space(24.0);
                            ui.label(
                                RichText::new("A quiet place to read.")
                                    .font(FontId::new(
                                        28.0,
                                        FontFamily::Name(theme::FONT_BODY.into()),
                                    ))
                                    .color(theme::TEXT),
                            );
                            ui.add_space(8.0);
                            ui.label(muted_label("EPUB · KEPUB · MOBI"));
                            ui.add_space(32.0);
                            if nb_button(ui, "Open book", true).clicked() {
                                open_request = true;
                            }
                            ui.add_space(40.0);
                        });

                        let recents = self.state.recents.clone();
                        if !recents.is_empty() {
                            column(ui, theme::CONTENT_MAX_WIDTH, |ui| {
                                ui.label(muted_label("RECENT"));
                                ui.add_space(12.0);
                                for recent in &recents {
                                    ui.horizontal(|ui| {
                                        let title_width = (ui.available_width() - 100.0).max(80.0);
                                        let label = ui
                                            .allocate_ui_with_layout(
                                                Vec2::new(title_width, 28.0),
                                                Layout::left_to_right(Align::Center),
                                                |ui| {
                                                    ui.set_min_width(title_width);
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(&recent.title)
                                                                .font(theme::body_font(
                                                                    theme::BODY_SIZE,
                                                                ))
                                                                .color(theme::TEXT),
                                                        )
                                                        .selectable(false)
                                                        .sense(Sense::click()),
                                                    )
                                                },
                                            )
                                            .inner;
                                        let clicked = label.clicked();
                                        label
                                            .on_hover_text(&recent.path)
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if clicked {
                                            open_path = Some(PathBuf::from(&recent.path));
                                        }
                                        if nb_button(ui, "Remove", false).clicked() {
                                            forget = Some(recent.path.clone());
                                        }
                                    });
                                    ui.add_space(4.0);
                                }
                            });
                        }

                        if let Some(message) = &self.error {
                            ui.add_space(24.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(message.as_str())
                                        .font(theme::label_font())
                                        .color(theme::BEAK),
                                );
                            });
                        }
                        ui.add_space(48.0);
                    });
            });

        if let Some(path) = forget {
            self.state.forget(&path);
            self.state.save();
        }
        if open_request {
            self.pick_file(&ctx);
        }
        if let Some(path) = open_path {
            self.open_book(path, &ctx);
        }
    }

    // -- About ------------------------------------------------------------

    fn about_ui(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(theme::BLACK))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("about")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                    column(ui, theme::CONTENT_MAX_WIDTH, |ui| {
                        ui.add_space(24.0);
                        ui.vertical_centered(|ui| {
                            barcode(ui, "*NOT FOR SALE*", 40.0);
                            ui.add_space(24.0);
                            ui.label(
                                RichText::new("epub-reader")
                                    .font(theme::heading_font(24.0))
                                    .color(theme::TEXT),
                            );
                            ui.label(muted_label(&format!(
                                "VERSION {} · A NETHERBASE TOOL",
                                env!("CARGO_PKG_VERSION")
                            )));
                        });
                        ui.add_space(16.0);
                        body_text(
                            ui,
                            "A single portable executable that reads EPUB, KEPUB and MOBI books. It makes no network requests and keeps your recents and reading positions on this machine only.",
                        );
                        ui.add_space(32.0);
                        ui.label(
                            RichText::new("Fonts")
                                .font(theme::heading_font(18.0))
                                .color(theme::TEXT),
                        );
                        ui.add_space(8.0);
                        for (name, licence) in [
                            (
                                "Chakra Petch",
                                include_str!("../brand/licences/OFL-ChakraPetch.txt"),
                            ),
                            (
                                "Share Tech Mono",
                                include_str!("../brand/licences/OFL-ShareTechMono.txt"),
                            ),
                            (
                                "Libre Barcode 39 Text",
                                include_str!("../brand/licences/OFL-LibreBarcode.txt"),
                            ),
                        ] {
                            ui.label(
                                RichText::new(name)
                                    .font(theme::body_font(theme::BODY_SIZE))
                                    .color(theme::TEXT),
                            );
                            ui.label(
                                RichText::new(licence.trim())
                                    .font(FontId::new(
                                        11.0,
                                        FontFamily::Name(theme::FONT_MONO.into()),
                                    ))
                                    .color(theme::MUTED),
                            );
                            ui.add_space(12.0);
                        }
                        ui.add_space(48.0);
                    });
                });
            });
    }

    // -- Reader -----------------------------------------------------------

    fn reader_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut toc_jump: Option<usize> = None;
        let mut chapter_delta: i64 = 0;

        let keys = ctx.input(|i| {
            let mut keys = Vec::new();
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                    && (modifiers.ctrl || modifiers.command)
                {
                    keys.push(*key);
                }
            }
            keys
        });
        for key in keys {
            match key {
                egui::Key::ArrowRight => chapter_delta += 1,
                egui::Key::ArrowLeft => chapter_delta -= 1,
                _ => {}
            }
        }

        let (show_toc, toc, current) = match &self.screen {
            Screen::Reader(reader) => (reader.show_toc, reader.book.toc.clone(), reader.chapter),
            _ => (false, Vec::new(), 0),
        };

        if show_toc {
            egui::Panel::left("reader_toc")
                .frame(Frame::NONE.fill(theme::BLACK))
                .exact_size(TOC_WIDTH)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add_space(12.0);
                    if let Some(texture) = self.texture_for(&ctx, "cover:__") {
                        ui.add(
                            Image::new(&texture)
                                .max_size(Vec2::new(ui.available_width() - 16.0, 260.0)),
                        );
                        ui.add_space(12.0);
                    }
                    ui.label(muted_label("CONTENTS"));
                    ui.add_space(8.0);
                    ScrollArea::vertical().id_salt("toc").show(ui, |ui| {
                        for node in &toc {
                            toc_node(ui, node, 0, current, &mut toc_jump);
                        }
                    });
                });
        }

        let chapters = match &self.screen {
            Screen::Reader(r) => r.book.chapters.len(),
            _ => 0,
        };

        egui::Panel::bottom("reader_bottom")
            .frame(
                Frame::NONE
                    .fill(theme::BLACK)
                    .inner_margin(Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                let fraction = self.reader_fraction();
                let chapter = match &self.screen {
                    Screen::Reader(r) => r.chapter,
                    _ => 0,
                };
                ui.horizontal(|ui| {
                    if nb_button(ui, "Previous", false).clicked() {
                        chapter_delta -= 1;
                    }
                    let overall = if chapters > 0 {
                        ((chapter as f32 + fraction) / chapters as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if nb_button(ui, "Next", false).clicked() {
                            chapter_delta += 1;
                        }
                        ui.add_space(12.0);
                        let width = ui.available_width().max(40.0);
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(width, 10.0), Sense::hover());
                        let bar = Rect::from_min_max(
                            Pos2::new(rect.left(), rect.center().y - 1.0),
                            Pos2::new(rect.right(), rect.center().y + 1.0),
                        );
                        ui.painter()
                            .rect_filled(bar, CornerRadius::ZERO, theme::LINE);
                        ui.painter().rect_filled(
                            Rect::from_min_size(bar.min, Vec2::new(bar.width() * overall, 2.0)),
                            CornerRadius::ZERO,
                            theme::BEAK,
                        );
                    });
                });
            });

        let drawn: Vec<LinkAction> = egui::CentralPanel::default()
            .frame(Frame::NONE.fill(theme::BLACK))
            .show(ui, |ui| {
                let (chapter_index, restore, pending_offset, base) = match &mut self.screen {
                    Screen::Reader(reader) => {
                        let chapter_index = reader.chapter;
                        let base = reader
                            .book
                            .chapters
                            .get(chapter_index)
                            .map(|c| c.base.clone())
                            .unwrap_or_default();
                        (
                            chapter_index,
                            reader.restore.take(),
                            reader.pending_offset.take(),
                            base,
                        )
                    }
                    _ => return Vec::new(),
                };

                let blocks = self
                    .blocks
                    .entry(chapter_index)
                    .or_insert_with(|| match &self.screen {
                        Screen::Reader(reader) => reader
                            .book
                            .chapters
                            .get(chapter_index)
                            .map(|c| parse_blocks(&c.html))
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    })
                    .clone();

                let mut area = ScrollArea::vertical()
                    .id_salt(("chapter", chapter_index))
                    .auto_shrink(false);
                if let Some(offset) = pending_offset {
                    area = area.vertical_scroll_offset(offset);
                }
                let column_width = self.state.column_width;
                let output = area.show(ui, |ui| {
                    column(ui, column_width, |ui| {
                        ui.add_space(32.0);
                        let actions = draw_blocks(ui, self, &base, chapter_index, &blocks);
                        ui.add_space(64.0);
                        actions
                    })
                });

                let scrollable = (output.content_size.y - output.inner_rect.height()).max(1.0);
                if let Screen::Reader(reader) = &mut self.screen {
                    if let Some(fraction) = restore {
                        let target = (fraction * scrollable).clamp(0.0, scrollable);
                        reader.pending_offset = Some(target);
                        ctx.request_repaint();
                    } else {
                        reader.fraction =
                            (output.state.clamped_offset().y / scrollable).clamp(0.0, 1.0);
                    }
                }
                output.inner
            })
            .inner;

        for action in drawn {
            match action {
                LinkAction::External(url) => {
                    let _ = open::that_detached(url);
                }
                LinkAction::Internal(target) => {
                    if let Screen::Reader(reader) = &mut self.screen {
                        let base = reader
                            .book
                            .chapters
                            .get(reader.chapter)
                            .map(|c| c.base.clone())
                            .unwrap_or_default();
                        if let Some(index) = find_chapter(&reader.book, &base, &target) {
                            let key = reader.book.path.to_string_lossy().to_string();
                            let pos = Position {
                                chapter: reader.chapter,
                                fraction: reader.fraction,
                            };
                            self.state.positions.insert(key, pos);
                            reader.chapter = index;
                            reader.restore = Some(0.0);
                        }
                    }
                }
            }
        }

        if let Some(jump) = toc_jump
            && let Screen::Reader(reader) = &mut self.screen
            && jump < reader.book.chapters.len()
        {
            let key = reader.book.path.to_string_lossy().to_string();
            let pos = Position {
                chapter: reader.chapter,
                fraction: reader.fraction,
            };
            self.state.positions.insert(key, pos);
            reader.chapter = jump;
            reader.restore = Some(0.0);
        }
        if chapter_delta != 0
            && let Screen::Reader(reader) = &mut self.screen
        {
            let max = reader.book.chapters.len().saturating_sub(1);
            let next = (reader.chapter as i64 + chapter_delta).clamp(0, max as i64) as usize;
            if next != reader.chapter {
                let key = reader.book.path.to_string_lossy().to_string();
                let pos = Position {
                    chapter: reader.chapter,
                    fraction: reader.fraction,
                };
                self.state.positions.insert(key, pos);
                reader.chapter = next;
                reader.restore = Some(0.0);
            }
        }
    }

    fn reader_fraction(&self) -> f32 {
        match &self.screen {
            Screen::Reader(r) => r.fraction,
            _ => 0.0,
        }
    }
}

fn find_chapter(book: &Book, base: &str, target: &str) -> Option<usize> {
    let key = crate::book::resolve_href(base, target);
    book.chapter_paths
        .iter()
        .position(|p| *p == key)
        .or_else(|| {
            book.chapter_paths
                .iter()
                .position(|p| p.ends_with(&format!("/{key}")))
        })
}

fn toc_node(ui: &mut Ui, node: &TocNode, depth: usize, current: usize, jump: &mut Option<usize>) {
    ui.horizontal(|ui| {
        ui.add_space(8.0 + depth as f32 * 16.0);
        let colour = if node.chapter == current {
            theme::BEAK
        } else {
            theme::MUTED
        };
        let response = ui.add(
            egui::Label::new(
                RichText::new(&node.label)
                    .font(theme::body_font(theme::BODY_SIZE - 1.0))
                    .color(colour),
            )
            .truncate()
            .selectable(false)
            .sense(Sense::click()),
        );
        let response = response.on_hover_text(&node.label);
        if response.clicked() {
            *jump = Some(node.chapter);
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand);
    });
    for child in &node.children {
        toc_node(ui, child, depth + 1, current, jump);
    }
}

// -- shared widgets -------------------------------------------------------

fn muted_label(text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .font(theme::label_font())
        .color(theme::MUTED)
}

fn column<R>(ui: &mut Ui, max_width: f32, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    const INSET: f32 = 24.0;
    let width = (ui.available_width() - INSET * 2.0)
        .max(160.0)
        .min(max_width);
    ui.vertical_centered(|ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        ui.with_layout(Layout::top_down(Align::Min), add_contents)
            .inner
    })
    .inner
}

#[derive(Clone, Copy)]
enum Icon {
    Open,
    Settings,
    Contents,
    Back,
    Minimise,
    Maximise,
    Restore,
    Close,
}

/// Flat 32 × 30 icon button, matching MD Viewer's toolbar.
fn icon_button(ui: &mut Ui, icon: Icon, active: bool) -> Response {
    // Sense drag as well as click so a drag that starts on a button does not
    // fall through to the title-bar drag region behind it.
    let (rect, response) = ui.allocate_exact_size(Vec2::new(32.0, 30.0), Sense::click_and_drag());
    let hovered = response.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, theme::LINE);
    }
    let colour = if active {
        theme::BEAK
    } else if hovered {
        theme::TEXT
    } else {
        theme::MUTED
    };
    draw_icon(ui.painter(), icon, rect, colour);
    if hovered {
        ui.ctx().request_repaint();
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn draw_icon(painter: &egui::Painter, icon: Icon, rect: Rect, colour: egui::Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.7, colour);
    match icon {
        Icon::Open => {
            painter.add(egui::Shape::line(
                vec![
                    Pos2::new(c.x - 8.0, c.y + 6.0),
                    Pos2::new(c.x - 8.0, c.y - 5.0),
                    Pos2::new(c.x - 2.0, c.y - 5.0),
                    Pos2::new(c.x, c.y - 2.0),
                    Pos2::new(c.x + 8.0, c.y - 2.0),
                    Pos2::new(c.x + 8.0, c.y + 6.0),
                    Pos2::new(c.x - 8.0, c.y + 6.0),
                ],
                stroke,
            ));
        }
        Icon::Settings => {
            painter.line_segment(
                [
                    Pos2::new(c.x - 8.0, c.y - 4.0),
                    Pos2::new(c.x + 8.0, c.y - 4.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 8.0, c.y + 4.0),
                    Pos2::new(c.x + 8.0, c.y + 4.0),
                ],
                stroke,
            );
            painter.circle_filled(Pos2::new(c.x + 3.0, c.y - 4.0), 2.4, colour);
            painter.circle_filled(Pos2::new(c.x - 3.0, c.y + 4.0), 2.4, colour);
        }
        Icon::Contents => {
            painter.line_segment(
                [
                    Pos2::new(c.x - 8.0, c.y - 5.0),
                    Pos2::new(c.x + 8.0, c.y - 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - 8.0, c.y), Pos2::new(c.x + 2.0, c.y)],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 8.0, c.y + 5.0),
                    Pos2::new(c.x + 5.0, c.y + 5.0),
                ],
                stroke,
            );
        }
        Icon::Back => {
            painter.line_segment(
                [Pos2::new(c.x + 7.0, c.y), Pos2::new(c.x - 7.0, c.y)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - 7.0, c.y), Pos2::new(c.x - 2.0, c.y - 5.0)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - 7.0, c.y), Pos2::new(c.x - 2.0, c.y + 5.0)],
                stroke,
            );
        }
        Icon::Minimise => {
            painter.line_segment(
                [Pos2::new(c.x - 7.0, c.y), Pos2::new(c.x + 7.0, c.y)],
                stroke,
            );
        }
        Icon::Maximise => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(12.0)),
                CornerRadius::ZERO,
                stroke,
                StrokeKind::Inside,
            );
        }
        Icon::Restore => {
            painter.rect_stroke(
                Rect::from_center_size(Pos2::new(c.x + 2.0, c.y + 2.0), Vec2::splat(11.0)),
                CornerRadius::ZERO,
                stroke,
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                Rect::from_center_size(Pos2::new(c.x - 2.0, c.y - 2.0), Vec2::splat(11.0)),
                CornerRadius::ZERO,
                stroke,
                StrokeKind::Inside,
            );
        }
        Icon::Close => {
            painter.line_segment(
                [
                    Pos2::new(c.x - 6.0, c.y - 6.0),
                    Pos2::new(c.x + 6.0, c.y + 6.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 6.0, c.y - 6.0),
                    Pos2::new(c.x - 6.0, c.y + 6.0),
                ],
                stroke,
            );
        }
    }
}

fn bar_text_button(ui: &mut Ui, label: &str) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::button_font(), theme::TEXT);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(32.0, 30.0), Sense::click_and_drag());
    let hovered = response.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, theme::LINE);
    }
    let colour = if hovered { theme::TEXT } else { theme::MUTED };
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
    if hovered {
        ui.ctx().request_repaint();
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn bar_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, 30.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        (rect.top() + 5.0)..=(rect.bottom() - 5.0),
        Stroke::new(1.0, theme::LINE),
    );
}

fn rule(ui: &mut Ui) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.top(),
        Stroke::new(1.0, theme::LINE),
    );
    ui.add_space(1.0);
}

fn setting_row<R>(ui: &mut Ui, label: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    const CONTROL_WIDTH: f32 = 94.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.set_min_height(32.0);
        ui.label(
            RichText::new(label.to_uppercase())
                .font(theme::label_font())
                .color(theme::TEXT),
        );
        let space = (ui.available_width() - CONTROL_WIDTH).max(0.0);
        ui.add_space(space);
        add_contents(ui)
    })
    .inner
}

fn stepper(ui: &mut Ui, value: &str, minus_enabled: bool, plus_enabled: bool) -> (bool, bool) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let minus = step_button(ui, "−", minus_enabled).clicked();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(46.0, 24.0), Sense::hover());
            ui.painter().rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, theme::LINE),
                StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                value,
                theme::button_font(),
                theme::TEXT,
            );
            let plus = step_button(ui, "+", plus_enabled).clicked();
            (minus, plus)
        })
        .inner
    })
    .inner
}

fn step_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(24.0, 24.0), sense);
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, theme::LINE);
    }
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, theme::LINE),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        theme::button_font(),
        if enabled { theme::TEXT } else { theme::MUTED },
    );
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn link_button(ui: &mut Ui, label: &str) -> Response {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_uppercase(), theme::button_font(), theme::BEAK);
    let size = galley.size() + Vec2::new(0.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let colour = if hovered { theme::TEXT } else { theme::BEAK };
    ui.painter()
        .galley(rect.left_top() + Vec2::new(0.0, 3.0), galley, colour);
    if hovered {
        let y = rect.bottom() - 2.0;
        ui.painter()
            .hline(rect.left()..=rect.right(), y, Stroke::new(1.0, theme::TEXT));
        ui.ctx().request_repaint();
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Southern Cross plus "Aotearoa New Zealand", per style guide §5.5.
fn nz_mark(ui: &mut Ui) {
    let height = 22.0;
    let width = height * 24.0 / 30.0;
    let text_width = 150.0;
    let (rect, _response) =
        ui.allocate_exact_size(Vec2::new(width + 8.0 + text_width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let stars: [[f32; 20]; 4] = [
        [
            12.0, 0.6, 12.8, 2.9, 15.2, 2.9, 13.2, 4.4, 14.0, 6.8, 12.0, 5.3, 10.0, 6.8, 10.8, 4.4,
            8.8, 2.9, 11.2, 2.9,
        ],
        [
            4.5, 10.1, 5.3, 12.4, 7.7, 12.4, 5.7, 13.9, 6.5, 16.3, 4.5, 14.8, 2.5, 16.3, 3.3, 13.9,
            1.3, 12.4, 3.7, 12.4,
        ],
        [
            19.0, 8.8, 19.6, 10.7, 21.6, 10.7, 20.0, 11.8, 20.6, 13.7, 19.0, 12.5, 17.4, 13.7,
            18.0, 11.8, 16.4, 10.7, 18.4, 10.7,
        ],
        [
            12.0, 21.7, 12.9, 24.3, 15.6, 24.3, 13.4, 25.9, 14.2, 28.6, 12.0, 27.0, 9.8, 28.6,
            10.6, 25.9, 8.4, 24.3, 11.1, 24.3,
        ],
    ];
    let scale = height / 30.0;
    for star in stars.iter() {
        let points: Vec<Pos2> = star
            .chunks(2)
            .map(|pair| Pos2::new(rect.min.x + pair[0] * scale, rect.min.y + pair[1] * scale))
            .collect();
        painter.add(egui::Shape::convex_polygon(
            points,
            theme::BEAK,
            egui::Stroke::NONE,
        ));
    }
    painter.text(
        Pos2::new(rect.min.x + width + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "AOTEAROA NEW ZEALAND",
        theme::label_font(),
        theme::MUTED,
    );
}

/// Hero barcode in Libre Barcode 39 Text, beak on black.
fn barcode(ui: &mut Ui, text: &str, size: f32) {
    ui.label(
        RichText::new(text)
            .font(FontId::new(
                size,
                FontFamily::Name(theme::FONT_BARCODE.into()),
            ))
            .color(theme::BEAK),
    );
}

/// Square, flat button per style guide §5.6.
fn nb_button(ui: &mut Ui, label: &str, primary: bool) -> Response {
    let text = label.to_uppercase();
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::button_font(), theme::TEXT);
    let size = galley.size() + Vec2::new(28.0, 18.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let (bg, fg) = if primary {
        if hovered {
            (theme::TEXT, theme::BLACK)
        } else {
            (theme::BEAK, theme::BLACK)
        }
    } else if hovered {
        (theme::BEAK, theme::BLACK)
    } else {
        (theme::BLACK, theme::BEAK)
    };
    let stroke = theme::BEAK;
    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    let text_pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(text_pos, galley, fg);
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if hovered {
        ui.ctx().request_repaint();
    }
    response
}

fn body_text(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::body_font(theme::BODY_SIZE))
            .color(theme::TEXT),
    );
}

// -- chapter drawing ------------------------------------------------------

fn draw_blocks(
    ui: &mut Ui,
    app: &mut ReaderApp,
    base: &str,
    chapter: usize,
    blocks: &[Block],
) -> Vec<LinkAction> {
    let mut view = DrawView {
        ui,
        app,
        base,
        chapter,
        actions: Vec::new(),
        indent: 0.0,
    };
    view.blocks(blocks);
    view.actions
}

struct DrawView<'a> {
    ui: &'a mut Ui,
    app: &'a mut ReaderApp,
    base: &'a str,
    chapter: usize,
    actions: Vec<LinkAction>,
    indent: f32,
}

impl DrawView<'_> {
    fn blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            if block.is_empty() {
                continue;
            }
            self.block(block);
        }
    }

    fn block(&mut self, block: &Block) {
        match block {
            Block::Heading {
                level,
                inlines,
                align,
            } => {
                let scale = match level {
                    1 => 1.6,
                    2 => 1.35,
                    3 => 1.2,
                    4 => 1.1,
                    5 => 1.0,
                    _ => 0.95,
                };
                self.ui.add_space(16.0);
                self.paragraph(inlines, *align, 0.0, scale, true);
                self.ui.add_space(8.0);
            }
            Block::Paragraph {
                inlines,
                align,
                first_indent,
            } => {
                self.paragraph(inlines, *align, *first_indent, 1.0, false);
                self.ui.add_space(10.0);
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                self.ui.add_space(6.0);
                for (i, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}.", start + i as u64)
                    } else {
                        "•".to_owned()
                    };
                    let indent = self.indent;
                    let font_size = self.app.state.font_size;
                    let app = &mut *self.app;
                    let base = self.base;
                    let chapter = self.chapter;
                    let mut item_actions = Vec::new();
                    let ui = &mut *self.ui;
                    ui.horizontal_top(|ui| {
                        ui.add_space(indent);
                        if *ordered {
                            ui.add_sized(
                                [28.0, 20.0],
                                egui::Label::new(
                                    RichText::new(marker)
                                        .font(theme::body_font(font_size))
                                        .color(theme::MUTED),
                                )
                                .selectable(false),
                            );
                        } else {
                            let (rect, _) =
                                ui.allocate_exact_size(Vec2::new(28.0, 20.0), Sense::hover());
                            ui.painter().rect_filled(
                                Rect::from_center_size(
                                    Pos2::new(rect.left() + 5.0, rect.top() + 10.0),
                                    Vec2::splat(4.0),
                                ),
                                CornerRadius::ZERO,
                                theme::MUTED,
                            );
                        }
                        ui.vertical(|ui| {
                            let mut view = DrawView {
                                ui,
                                app,
                                base,
                                chapter,
                                actions: Vec::new(),
                                indent: 0.0,
                            };
                            view.blocks(item);
                            item_actions = view.actions;
                        });
                    });
                    self.actions.append(&mut item_actions);
                }
                self.ui.add_space(6.0);
            }
            Block::Quote(inner) => {
                self.ui.add_space(8.0);
                let top = self.ui.cursor().min.y;
                let indent = self.indent;
                let app = &mut *self.app;
                let base = self.base;
                let chapter = self.chapter;
                let mut inner_actions = Vec::new();
                let ui = &mut *self.ui;
                ui.horizontal_top(|ui| {
                    ui.add_space(indent + 20.0);
                    ui.vertical(|ui| {
                        let mut view = DrawView {
                            ui,
                            app,
                            base,
                            chapter,
                            actions: Vec::new(),
                            indent: 0.0,
                        };
                        view.blocks(inner);
                        inner_actions = view.actions;
                    });
                });
                let bottom = self.ui.cursor().min.y;
                let x = self.ui.min_rect().left() + indent + 10.0;
                self.ui
                    .painter()
                    .vline(x, top..=bottom, Stroke::new(1.0, theme::LINE));
                self.actions.append(&mut inner_actions);
                self.ui.add_space(8.0);
            }
            Block::Pre(text) => {
                self.ui.add_space(8.0);
                self.ui.add_space(self.indent);
                self.ui.label(
                    RichText::new(text)
                        .font(FontId::new(
                            self.app.state.font_size * 0.85,
                            FontFamily::Name(theme::FONT_MONO.into()),
                        ))
                        .color(theme::TEXT),
                );
                self.ui.add_space(8.0);
            }
            Block::Image { src, alt } => {
                self.ui.add_space(8.0);
                self.image(src, alt);
                self.ui.add_space(8.0);
            }
            Block::Rule => {
                self.ui.add_space(12.0);
                let rect = self.ui.available_rect_before_wrap();
                let y = rect.top() + 1.0;
                self.ui.painter().hline(
                    rect.left() + self.indent..=rect.right(),
                    y,
                    Stroke::new(1.0, theme::LINE),
                );
                self.ui.add_space(14.0);
            }
            Block::Table(rows) => {
                self.ui.add_space(8.0);
                let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                let chapter = self.chapter;
                let base = self.base.to_owned();
                let mut actions = Vec::new();
                egui::Grid::new(("table", chapter, rows.len()))
                    .num_columns(cols)
                    .spacing([16.0, 8.0])
                    .show(self.ui, |ui| {
                        for row in rows {
                            for cell in row {
                                ui.vertical(|ui| {
                                    let mut view = DrawView {
                                        ui,
                                        app: self.app,
                                        base: &base,
                                        chapter,
                                        actions: Vec::new(),
                                        indent: 0.0,
                                    };
                                    view.blocks(cell);
                                    actions.append(&mut view.actions);
                                });
                            }
                            ui.end_row();
                        }
                    });
                self.actions.append(&mut actions);
                self.ui.add_space(8.0);
            }
        }
    }

    fn paragraph(
        &mut self,
        inlines: &[Inline],
        align: HtmlAlign,
        first_indent: f32,
        scale: f32,
        heading: bool,
    ) {
        let font_size = self.app.state.font_size * scale;
        let line_height = self.app.state.line_height;
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        job.halign = match align {
            HtmlAlign::Left => Align::Min,
            HtmlAlign::Center => Align::Center,
            HtmlAlign::Right => Align::Max,
            HtmlAlign::Justify => Align::Min,
        };
        job.justify = align == HtmlAlign::Justify;
        job.break_on_newline = true;

        let mut link_ranges: Vec<(Range<usize>, String)> = Vec::new();
        for inline in inlines {
            match inline {
                Inline::Text { text, style, href } => {
                    let start = job.text.len();
                    job.text.push_str(text);
                    let end = job.text.len();
                    let mut size = font_size * style.scale;
                    if style.superscript || style.subscript {
                        size *= 0.7;
                    }
                    let family = if style.code {
                        FontFamily::Name(theme::FONT_MONO.into())
                    } else if heading {
                        FontFamily::Name(theme::FONT_DISPLAY.into())
                    } else {
                        FontFamily::Name(theme::FONT_BODY.into())
                    };
                    let underline = if href.is_some() {
                        Stroke::new(1.0, theme::MUTED)
                    } else if style.underline {
                        Stroke::new(1.0, theme::TEXT)
                    } else {
                        Stroke::NONE
                    };
                    let format = egui::text::TextFormat {
                        font_id: FontId::new(size, family),
                        color: theme::TEXT,
                        italics: style.italic,
                        underline,
                        strikethrough: if style.strike {
                            Stroke::new(1.0, theme::TEXT)
                        } else {
                            Stroke::NONE
                        },
                        line_height: Some(size * line_height),
                        ..Default::default()
                    };
                    job.sections.push(egui::text::LayoutSection {
                        leading_space: if start == 0 { first_indent } else { 0.0 },
                        byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(end),
                        format,
                    });
                    if let Some(href) = href {
                        let char_start = job.text[..start].chars().count();
                        let char_end = char_start + text.chars().count();
                        link_ranges.push((char_start..char_end, href.clone()));
                    }
                }
                Inline::Break => {
                    let start = job.text.len();
                    job.text.push('\n');
                    job.sections.push(egui::text::LayoutSection {
                        leading_space: 0.0,
                        byte_range: egui::text::ByteIndex(start)
                            ..egui::text::ByteIndex(job.text.len()),
                        format: egui::text::TextFormat {
                            font_id: theme::body_font(font_size),
                            color: theme::TEXT,
                            line_height: Some(font_size * line_height),
                            ..Default::default()
                        },
                    });
                }
                Inline::Image { src: _, alt } => {
                    let start = job.text.len();
                    job.text.push_str(&format!("[{alt}]"));
                    job.sections.push(egui::text::LayoutSection {
                        leading_space: 0.0,
                        byte_range: egui::text::ByteIndex(start)
                            ..egui::text::ByteIndex(job.text.len()),
                        format: egui::text::TextFormat {
                            font_id: theme::body_font(font_size),
                            color: theme::MUTED,
                            italics: true,
                            line_height: Some(font_size * line_height),
                            ..Default::default()
                        },
                    });
                }
            }
        }
        if job.text.trim().is_empty() {
            return;
        }
        let width = (self.ui.available_width() - self.indent).max(80.0);
        job.wrap.max_width = width;
        let response = self.ui.add(
            egui::Label::new(job.clone())
                .selectable(false)
                .sense(Sense::click()),
        );
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let galley = self.ui.ctx().fonts_mut(|f| f.layout_job(job.clone()));
            let cursor = galley.cursor_from_pos(pos - response.rect.min);
            for (range, href) in &link_ranges {
                if range.contains(&cursor.index.0) {
                    self.actions.push(
                        if href.starts_with("http://")
                            || href.starts_with("https://")
                            || href.starts_with("mailto:")
                        {
                            LinkAction::External(href.clone())
                        } else {
                            LinkAction::Internal(href.clone())
                        },
                    );
                    break;
                }
            }
        }
    }

    fn image(&mut self, src: &str, alt: &str) {
        let key = if src.starts_with("recindex:") || src.starts_with("data:") {
            src.to_owned()
        } else {
            crate::book::resolve_href(self.base, src)
        };
        let ctx = self.ui.ctx().clone();
        let texture = self.app.texture_for(&ctx, &key);
        match texture {
            Some(texture) => {
                let width = (self.ui.available_width() - self.indent).max(80.0);
                let [sx, sy] = texture.size();
                let (sx, sy) = (sx as f32, sy as f32);
                let aspect = sy / sx.max(1.0);
                let draw_width = width.min(sx);
                let max = Vec2::new(draw_width, (draw_width * aspect).min(900.0));
                self.ui.add(Image::new(&texture).max_size(max));
            }
            None => {
                self.ui.label(
                    RichText::new(format!(
                        "[image: {}]",
                        if alt.is_empty() { src } else { alt }
                    ))
                    .font(theme::body_font(self.app.state.font_size * 0.9))
                    .color(theme::MUTED)
                    .italics(),
                );
            }
        }
    }
}

pub fn window_icon() -> Option<Arc<egui::IconData>> {
    let img = image::load_from_memory(WINDOW_ICON).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = ColorImage::from_rgba_unmultiplied(size, rgba.as_flat_samples().as_slice());
    Some(Arc::new(egui::IconData {
        rgba: color.as_raw().to_vec(),
        width: size[0] as u32,
        height: size[1] as u32,
    }))
}
