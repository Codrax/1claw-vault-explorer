// Material You (M3) style theming for egui.
//
// Builds tonal palettes from a single seed color, the same idea as Android's
// dynamic color. Real M3 uses the HCT color space; OKLCH is close enough for
// our purposes and way less code. Tone 0..100 maps to L* like in M3.

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, TextStyle};

pub const DEFAULT_SEED: [u8; 3] = [0xE8, 0x5D, 0x3A];

/// A handful of seeds for the picker - "wallpaper" colors basically.
pub const SEEDS: &[([u8; 3], &str)] = &[
    (DEFAULT_SEED, "Claw orange"),
    ([0x67, 0x50, 0xA4], "Violet"),
    ([0x1E, 0x88, 0xE5], "Blue"),
    ([0x00, 0x89, 0x7B], "Teal"),
    ([0x43, 0xA0, 0x47], "Green"),
    ([0xD8, 0x1B, 0x60], "Pink"),
    ([0x8D, 0x6E, 0x63], "Mocha"),
];

#[derive(Clone, Copy, Debug)]
pub struct Scheme {
    pub dark: bool,
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,
    pub tertiary: Color32,
    pub tertiary_container: Color32,
    pub on_tertiary_container: Color32,
    pub error: Color32,
    pub on_error: Color32,
    pub error_container: Color32,
    pub on_error_container: Color32,
    pub surface: Color32,
    pub surface_container_low: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub outline: Color32,
    pub outline_variant: Color32,
}

#[derive(Clone, Copy)]
pub enum Role {
    Primary,
    Secondary,
    Tertiary,
    Error,
    Neutral,
}

// --- color math ---

fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB -> (chroma, hue in radians) in OKLCH
fn oklch_of(rgb: [u8; 3]) -> (f32, f32) {
    let [r, g, b] = rgb.map(to_linear);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let bb = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    ((a * a + bb * bb).sqrt(), bb.atan2(a))
}

fn oklch_to_rgb(lightness: f32, c: f32, h: f32) -> Option<[f32; 3]> {
    let (a, b) = (c * h.cos(), c * h.sin());
    let l = (lightness + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (lightness - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (lightness - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    let rgb = [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ];
    rgb.iter().all(|v| (-0.0001..=1.0001).contains(v)).then_some(rgb)
}

/// One color out of a tonal palette. Chroma gets pulled in until it fits sRGB.
fn tone(hue: f32, chroma: f32, t: f32) -> Color32 {
    // M3 tone is CIE L*. For neutrals OKLab L == cbrt(Y), which works out to:
    let lightness = if t > 8.0 { (t + 16.0) / 116.0 } else { (t / 903.3).cbrt() };
    let (mut lo, mut hi) = (0.0, chroma);
    let mut best = oklch_to_rgb(lightness, 0.0, hue).unwrap_or([lightness; 3]);
    if let Some(rgb) = oklch_to_rgb(lightness, chroma, hue) {
        best = rgb;
    } else {
        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            match oklch_to_rgb(lightness, mid, hue) {
                Some(rgb) => {
                    best = rgb;
                    lo = mid;
                }
                None => hi = mid,
            }
        }
    }
    let [r, g, b] = best.map(|v| (from_linear(v.clamp(0.0, 1.0)) * 255.0).round() as u8);
    Color32::from_rgb(r, g, b)
}

impl Scheme {
    pub fn from_seed(seed: [u8; 3], dark: bool) -> Self {
        let (c, h) = oklch_of(seed);
        // "tonal spot" style: keep primary lively, everything else calmer
        let p = |t| tone(h, c.clamp(0.09, 0.16), t);
        let s = |t| tone(h, 0.035, t);
        let ter = |t| tone(h + 1.05, 0.07, t); // ~60 deg around the wheel
        let n = |t| tone(h, 0.008, t);
        let nv = |t| tone(h, 0.018, t);
        let e = |t| tone(0.47, 0.17, t); // red

        if dark {
            Self {
                dark,
                primary: p(80.0),
                on_primary: p(20.0),
                primary_container: p(30.0),
                on_primary_container: p(90.0),
                secondary_container: s(30.0),
                on_secondary_container: s(90.0),
                tertiary: ter(80.0),
                tertiary_container: ter(30.0),
                on_tertiary_container: ter(90.0),
                error: e(80.0),
                on_error: e(20.0),
                error_container: e(30.0),
                on_error_container: e(90.0),
                surface: n(6.0),
                surface_container_low: n(10.0),
                surface_container: n(12.0),
                surface_container_high: n(17.0),
                surface_container_highest: n(22.0),
                on_surface: n(90.0),
                on_surface_variant: nv(80.0),
                outline: nv(60.0),
                outline_variant: nv(30.0),
            }
        } else {
            Self {
                dark,
                primary: p(40.0),
                on_primary: p(100.0),
                primary_container: p(90.0),
                on_primary_container: p(10.0),
                secondary_container: s(90.0),
                on_secondary_container: s(10.0),
                tertiary: ter(40.0),
                tertiary_container: ter(90.0),
                on_tertiary_container: ter(10.0),
                error: e(40.0),
                on_error: e(100.0),
                error_container: e(90.0),
                on_error_container: e(10.0),
                surface: n(98.0),
                surface_container_low: n(96.0),
                surface_container: n(94.0),
                surface_container_high: n(92.0),
                surface_container_highest: n(90.0),
                on_surface: n(10.0),
                on_surface_variant: nv(30.0),
                outline: nv(50.0),
                outline_variant: nv(80.0),
            }
        }
    }

    pub fn container(&self, r: Role) -> (Color32, Color32) {
        match r {
            Role::Primary => (self.primary_container, self.on_primary_container),
            Role::Secondary => (self.secondary_container, self.on_secondary_container),
            Role::Tertiary => (self.tertiary_container, self.on_tertiary_container),
            Role::Error => (self.error_container, self.on_error_container),
            Role::Neutral => (self.surface_container_highest, self.on_surface_variant),
        }
    }

    // --- buttons, roughly the M3 button family ---

    pub fn filled(&self, text: &str) -> egui::Button<'static> {
        egui::Button::new(RichText::new(text).strong().color(self.on_primary))
            .fill(self.primary)
            .corner_radius(CornerRadius::same(20))
            .min_size(egui::vec2(0.0, 40.0))
    }

    pub fn tonal(&self, text: &str) -> egui::Button<'static> {
        egui::Button::new(RichText::new(text).color(self.on_secondary_container))
            .fill(self.secondary_container)
            .corner_radius(CornerRadius::same(20))
            .min_size(egui::vec2(0.0, 36.0))
    }

    pub fn danger(&self, text: &str) -> egui::Button<'static> {
        egui::Button::new(RichText::new(text).strong().color(self.on_error))
            .fill(self.error)
            .corner_radius(CornerRadius::same(20))
            .min_size(egui::vec2(0.0, 36.0))
    }

    pub fn text_button(&self, text: &str, color: Color32) -> egui::Button<'static> {
        egui::Button::new(RichText::new(text).color(color)).frame(false).min_size(egui::vec2(0.0, 32.0))
    }

    /// Elevated-ish card. Higher level = lighter container.
    pub fn card(&self, level: u8) -> egui::Frame {
        let fill = match level {
            0 => self.surface_container_low,
            1 => self.surface_container,
            2 => self.surface_container_high,
            _ => self.surface_container_highest,
        };
        egui::Frame::new().fill(fill).corner_radius(CornerRadius::same(20)).inner_margin(Margin::same(16))
    }

    pub fn panel(&self, fill: Color32) -> egui::Frame {
        egui::Frame::new().fill(fill).inner_margin(Margin::symmetric(12, 10))
    }
}

/// Little rounded label, like an M3 assist chip.
pub fn chip(ui: &mut egui::Ui, s: &Scheme, text: &str, role: Role) {
    let (bg, fg) = s.container(role);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(fg));
        });
}

/// Swatch button for the color picker.
pub fn swatch(ui: &mut egui::Ui, color: Color32, selected: bool, ring: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
    let p = ui.painter();
    let r = if resp.hovered() { 13.0 } else { 12.0 };
    p.circle_filled(rect.center(), r, color);
    if selected {
        p.circle_stroke(rect.center(), 14.5, Stroke::new(2.0f32, ring));
    }
    resp
}

/// Full width list / nav drawer item: rounded pill, left aligned label.
pub fn nav_item(ui: &mut egui::Ui, sc: &Scheme, text: &str, selected: bool, height: f32) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let bg = if selected {
            sc.secondary_container
        } else if resp.hovered() {
            mix(sc.surface_container_low, sc.on_surface, 0.08)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same((height / 2.0) as u8), bg);
        let fg = if selected { sc.on_secondary_container } else { sc.on_surface_variant };
        let font = egui::TextStyle::Button.resolve(ui.style());
        let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
        let pos = egui::pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().with_clip_rect(rect.shrink(4.0)).galley(pos, galley, fg);
    }
    resp
}

/// Rounded tonal tile with an emoji in it, used as the "app icon".
pub fn icon_tile(ui: &mut egui::Ui, sc: &Scheme, icon: &str, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same((size * 0.3) as u8), sc.primary_container);
    p.text(rect.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(size * 0.5), sc.on_primary_container);
    resp
}

pub fn scheme(ctx: &egui::Context) -> Scheme {
    ctx.data(|d| d.get_temp::<Scheme>(egui::Id::new("m3_scheme")))
        .unwrap_or_else(|| Scheme::from_seed(DEFAULT_SEED, true))
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn apply(ctx: &egui::Context, seed: [u8; 3], dark: bool) {
    let s = Scheme::from_seed(seed, dark);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("m3_scheme"), s));

    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = s.surface;
    v.window_fill = s.surface_container_high;
    v.window_stroke = Stroke::NONE;
    v.window_corner_radius = CornerRadius::same(28);
    v.menu_corner_radius = CornerRadius::same(16);
    v.window_shadow = egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 110 } else { 40 }) };
    v.popup_shadow = egui::Shadow { offset: [0, 3], blur: 12, spread: 0, color: Color32::from_black_alpha(if dark { 90 } else { 30 }) };
    v.extreme_bg_color = s.surface_container_highest;
    v.faint_bg_color = mix(s.surface, s.surface_container_highest, 0.45);
    v.code_bg_color = s.surface_container_highest;
    v.hyperlink_color = s.primary;
    v.error_fg_color = s.error;
    v.warn_fg_color = s.tertiary;
    v.selection.bg_fill = s.secondary_container;
    v.selection.stroke = Stroke::new(1.5f32, s.primary);
    v.text_cursor.stroke = Stroke::new(2.0f32, s.primary);
    v.striped = false;

    let fg = Stroke::new(1.0f32, s.on_surface);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = s.surface;
    w.noninteractive.weak_bg_fill = s.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0f32, s.outline_variant);
    w.noninteractive.fg_stroke = fg;
    w.noninteractive.corner_radius = CornerRadius::same(12);

    let base = s.surface_container_highest;
    for (state, overlay) in [
        (&mut w.inactive, 0.0),
        (&mut w.hovered, 0.08),
        (&mut w.active, 0.14),
        (&mut w.open, 0.08),
    ] {
        state.bg_fill = mix(base, s.on_surface, overlay);
        state.weak_bg_fill = mix(base, s.on_surface, overlay);
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = fg;
        state.corner_radius = CornerRadius::same(18);
        state.expansion = 0.0;
    }
    // hovered things get a hint of the accent so it doesn't feel flat
    w.hovered.fg_stroke = Stroke::new(1.0f32, s.on_surface);
    w.hovered.bg_stroke = Stroke::new(1.0f32, s.outline_variant);
    ctx.set_visuals(v);

    ctx.style_mut(|st| {
        use egui::{FontFamily, FontId};
        st.text_styles = [
            (TextStyle::Heading, FontId::new(22.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.5, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace)),
        ]
        .into();
        st.spacing.item_spacing = egui::vec2(10.0, 8.0);
        st.spacing.button_padding = egui::vec2(14.0, 6.0);
        st.spacing.interact_size = egui::vec2(40.0, 30.0);
        st.spacing.window_margin = Margin::same(22);
        st.spacing.menu_margin = Margin::same(10);
        st.spacing.indent = 16.0;
        st.spacing.scroll = egui::style::ScrollStyle::floating();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_get_lighter() {
        let (c, h) = oklch_of(DEFAULT_SEED);
        let dark = tone(h, c, 20.0);
        let light = tone(h, c, 90.0);
        let sum = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        assert!(sum(light) > sum(dark));
        assert_eq!(tone(h, 0.0, 100.0), Color32::WHITE);
    }

    #[test]
    fn schemes_build_for_all_seeds() {
        for (seed, _) in SEEDS {
            let d = Scheme::from_seed(*seed, true);
            let l = Scheme::from_seed(*seed, false);
            assert_ne!(d.surface, l.surface);
        }
    }
}
