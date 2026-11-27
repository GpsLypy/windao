use crate::dock::{CapsuleShape, CapsuleSkin, DockEdge};
use eframe::egui::{
    self, Color32, CornerRadius, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Palette {
    pub surface: Color32,
    pub card: Color32,
    pub border: Color32,
    pub caption: Color32,
    pub ink: Color32,
    pub signal: Color32,
    pub selected: Color32,
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Palette {
    pub fn for_skin(skin: CapsuleSkin, cover: Option<Color32>) -> Self {
        let [surface, card, border, caption, ink, signal, selected] = match skin {
            CapsuleSkin::Classic => [
                0x000000, 0x1d1d20, 0x333338, 0xb6bcc1, 0xf5f5f5, 0x77a6ff, 0x2a3652,
            ],
            CapsuleSkin::Cream => [
                0xf6eddb, 0xfff9ef, 0xd5c5a5, 0x695c4a, 0x342d24, 0x81542c, 0xead8b8,
            ],
            CapsuleSkin::GlassLight => [
                0xe4edf3, 0xf4f8fa, 0xb1c4cd, 0x465e6c, 0x1e333e, 0x21627c, 0xc0dbe8,
            ],
            CapsuleSkin::GlassDark => [
                0x202933, 0x323e49, 0x52616d, 0xb7c7d2, 0xf3f7fa, 0x99d9ed, 0x3c555f,
            ],
            CapsuleSkin::Cloud => [
                0x222238, 0x323247, 0x51536a, 0xc2c5dd, 0xf7f6ff, 0xb9c9ff, 0x454660,
            ],
        }
        .map(rgb);
        let mut p = Self {
            surface,
            card,
            border,
            caption,
            ink,
            signal,
            selected,
        };
        if skin == CapsuleSkin::Cloud
            && let Some(c) = cover
        {
            p.surface = rgb(0x171a24).lerp_to_gamma(c, 0.17);
            p.card = rgb(0x292c36).lerp_to_gamma(c, 0.20);
            p.border = rgb(0x4c5061).lerp_to_gamma(c, 0.24);
            p.signal = Color32::WHITE.lerp_to_gamma(c, 0.32);
            p.selected = p.card.lerp_to_gamma(c, 0.18);
        }
        p
    }
    pub fn apply(self, ui: &mut Ui) {
        let v = &mut ui.style_mut().visuals;
        v.override_text_color = Some(self.ink);
        v.weak_text_color = Some(self.caption);
        v.hyperlink_color = self.signal;
        v.panel_fill = self.card;
        v.window_fill = self.surface;
        v.window_stroke.color = self.border;
        v.extreme_bg_color = self.card;
        v.selection.bg_fill = self.selected;
        v.selection.stroke = Stroke::new(1.0_f32, self.signal);
        v.widgets.noninteractive.fg_stroke.color = self.caption;
        for (widget, fill, outline) in [
            (&mut v.widgets.inactive, self.card, self.border),
            (
                &mut v.widgets.hovered,
                self.card.lerp_to_gamma(self.ink, 0.08),
                self.border,
            ),
            (&mut v.widgets.active, self.selected, self.ink),
            (&mut v.widgets.open, self.card, self.border),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = Stroke::new(1.0_f32, outline);
            widget.fg_stroke.color = self.ink;
        }
    }
    pub fn from_ui(ui: &Ui) -> Self {
        let v = ui.visuals();
        Self {
            surface: v.window_fill,
            card: v.panel_fill,
            border: v.window_stroke.color,
            caption: v.weak_text_color(),
            ink: v.text_color(),
            signal: v.hyperlink_color,
            selected: v.selection.bg_fill,
        }
    }
}

pub(super) fn corners(radius: u8, shape: CapsuleShape, edge: DockEdge) -> CornerRadius {
    let mut c = CornerRadius::same(radius);
    if shape == CapsuleShape::Notch {
        match edge {
            DockEdge::Top => {
                c.nw = 0;
                c.ne = 0;
            }
            DockEdge::Bottom => {
                c.sw = 0;
                c.se = 0;
            }
            DockEdge::Left => {
                c.nw = 0;
                c.sw = 0;
            }
            DockEdge::Right => {
                c.ne = 0;
                c.se = 0;
            }
        }
    }
    c
}

#[cfg(any(windows, test))]
pub(super) fn cover_color(image: &egui::ColorImage) -> Option<Color32> {
    let mut sum = [0u32; 3];
    let mut weight = 0;
    for pixel in image
        .pixels
        .iter()
        .step_by(image.pixels.len().div_ceil(64).max(1))
    {
        if pixel.a() < 180 {
            continue;
        }
        let [r, g, b, _] = pixel.to_array();
        let delta = u32::from(r.max(g).max(b) - r.min(g).min(b));
        let w = delta + 12;
        for (value, c) in sum.iter_mut().zip([r, g, b]) {
            *value += u32::from(c) * w;
        }
        weight += w;
    }
    (weight > 0).then(|| {
        Color32::from_rgb(
            (sum[0] / weight) as u8,
            (sum[1] / weight) as u8,
            (sum[2] / weight) as u8,
        )
    })
}

// A convex fan keeps the cloud highlight entirely within each rounded or flat edge.
pub(super) fn cloud_glow(
    p: &Painter,
    rect: Rect,
    corners: CornerRadius,
    pointer: Option<Pos2>,
    tint: Color32,
    outline: bool,
) {
    let pointer = pointer
        .filter(|point| rect.contains(*point))
        .unwrap_or(rect.center());
    let origin = rect.center().lerp(pointer, 0.65);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(
        origin,
        Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), 62),
    );
    for (corner, r, start, offset) in [
        (
            rect.right_top(),
            corners.ne,
            -90.0_f32,
            egui::vec2(-1.0, 1.0),
        ),
        (rect.right_bottom(), corners.se, 0.0, egui::vec2(-1.0, -1.0)),
        (rect.left_bottom(), corners.sw, 90.0, egui::vec2(1.0, -1.0)),
        (rect.left_top(), corners.nw, 180.0, egui::vec2(1.0, 1.0)),
    ] {
        let r = f32::from(r);
        for i in 0..=6 {
            let at = corner + offset * r + Vec2::angled((start + i as f32 * 15.0).to_radians()) * r;
            mesh.colored_vertex(at, Color32::TRANSPARENT);
        }
    }
    let n = mesh.vertices.len() as u32 - 1;
    for i in 1..=n {
        mesh.add_triangle(0, i, 1 + i % n);
    }
    p.add(mesh);
    if outline {
        p.rect_stroke(
            rect,
            corners,
            Stroke::new(1.0_f32, tint.gamma_multiply(0.30)),
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ui::{
            Page,
            capsule::{self, CapsuleOptions, CapsuleState},
            dock_settings::Editor,
        },
        workspace::{Settings, Store},
    };
    use eframe::egui::{Event, PointerButton, Pos2, RawInput, vec2};
    #[test]
    fn settings_clicks_persist_and_both_surfaces_render_the_selected_skin_and_shape() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        let mut render = |events| {
            ctx.run_ui(
                RawInput {
                    events,
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900., 720.))),
                    ..Default::default()
                },
                |ui| {
                    editor.appearance(ui, &mut settings, &[]);
                },
            )
        };
        for label in ["经典黑", "奶油", "胶囊", "刘海", "收起时显示猫咪伙伴"] {
            let out = render(vec![]);
            let bounds = out
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .into_iter()
                .find_map(|(_, node)| {
                    (node.label() == Some(label) || node.value() == Some(label))
                        .then(|| node.bounds())
                        .flatten()
                })
                .unwrap_or_else(|| panic!("missing setting {label}"));
            let pos = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.) as f32,
                ((bounds.y0 + bounds.y1) / 2.) as f32,
            );
            for pressed in [true, false] {
                render(vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        pressed,
                        button: PointerButton::Primary,
                        modifiers: Default::default(),
                    },
                ]);
            }
        }
        assert_eq!(settings.dock.skin, CapsuleSkin::Cream);
        assert_eq!(settings.dock.shape, CapsuleShape::Notch);
        assert!(settings.dock.companion);
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(tmp.path().into()).unwrap();
        store
            .update(|ws| {
                ws.settings = settings;
                Ok(())
            })
            .unwrap();
        drop(store);
        let mut store = Store::open(tmp.path().into()).unwrap();
        for expanded in [false, true] {
            let ctx = egui::Context::default();
            let compact = capsule::compact_size(&store, &store.state().settings.dock, None);
            assert_eq!(compact.x, 152.);
            let size = if expanded { vec2(680., 318.) } else { compact };
            let screen = Rect::from_min_size(Pos2::ZERO, size);
            let mut state = CapsuleState::default();
            let mut workbench = crate::ui::Workbench::default();
            #[cfg(windows)]
            let mut sources = crate::ui::FeatureSources::new(ctx.clone());
            let out = ctx.run_ui(
                RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    capsule::draw(
                        ui,
                        &mut store,
                        &mut state,
                        CapsuleOptions {
                            dock: DockEdge::Top,
                            expanded,
                            pinned: false,
                            avoiding: false,
                            hidden: false,
                            reduce_motion: true,
                            collapsed_size: compact,
                            collapsed_center: None,
                            scale: 1.,
                            lyrics: None,
                            clock_24h: true,
                            minimal: false,
                            pages: &[Page::Calendar],
                        },
                        &mut workbench,
                        #[cfg(windows)]
                        &mut sources,
                    );
                },
            );
            let fill = Palette::for_skin(CapsuleSkin::Cream, None).surface;
            if expanded {
                assert!(out.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh)
                    if mesh.vertices.first().is_some_and(|vertex| vertex.pos == screen.left_top() && vertex.color == fill)
                        && mesh.vertices.get(1).is_some_and(|vertex| vertex.pos == screen.right_top())
                        && mesh.vertices.iter().any(|vertex| vertex.pos == screen.left_top() + vec2(28.0, 28.0))
                        && mesh.vertices.iter().any(|vertex| vertex.pos == screen.right_top() + vec2(-28.0, 28.0)))));
            } else {
                assert!(out.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(bg)
                    if bg.rect == screen && bg.fill == fill && bg.corner_radius.nw == 0 && bg.corner_radius.sw > 0)));
            }
        }
    }

    #[test]
    fn cloud_uses_art_pixels_and_ignores_fully_transparent_covers() {
        let art = egui::ColorImage::filled([4, 4], Color32::from_rgb(188, 44, 59));
        let color = cover_color(&art);
        assert_eq!(color, Some(Color32::from_rgb(188, 44, 59)));
        assert_ne!(
            Palette::for_skin(CapsuleSkin::Cloud, None).surface,
            Palette::for_skin(CapsuleSkin::Cloud, color).surface
        );
        assert!(cover_color(&egui::ColorImage::filled([4, 4], Color32::TRANSPARENT)).is_none());
    }
}
