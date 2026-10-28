//! Bound chrome rasterization to the areas outside native terminal cells.
use starkit::{
    native_surface::{PixelRect, Primitive, Surface},
    terminal_graphics::protocol::{Component, Rect, Viewport},
};

fn intersection(a: PixelRect, b: PixelRect) -> Option<PixelRect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then(|| PixelRect::new(x, y, right - x, bottom - y))
}
fn crop(surface: &Surface, area: PixelRect) -> Option<Surface> {
    // Cropping a text box would alter shaping, centering or wrapping. Preserve
    // the monolithic path whenever a future control crosses a tile boundary.
    if surface.nodes.iter().any(|node| {
        intersection(node.rect(), area)
            .is_some_and(|r| r != node.rect() && !matches!(node, Primitive::Fill { radius: 0, .. }))
    }) {
        return None;
    }
    let mut tile = surface.clone();
    tile.width = area.width;
    tile.height = area.height;
    tile.retain_nodes(|node| intersection(node.rect(), area).is_some());
    for node in &mut tile.nodes {
        let rect = intersection(node.rect(), area)?;
        *node.rect_mut() =
            PixelRect::new(rect.x - area.x, rect.y - area.y, rect.width, rect.height);
    }
    tile.hits.retain_mut(|hit| {
        if let Some(rect) = intersection(hit.rect, area) {
            hit.rect = PixelRect::new(rect.x - area.x, rect.y - area.y, rect.width, rect.height);
            true
        } else {
            false
        }
    });
    for rule in &mut tile.text_rules {
        rule.end = rule.end.saturating_sub(area.x);
    }
    Some(tile)
}

/// Keep the shared KIT cache useful at large window sizes. Terminal holes are
/// already painted by the parent; retaining full-window chrome underneath them
/// wastes raster time and pushes the useful chrome out of the bounded cache.
pub fn chrome(
    surface: Surface,
    viewport: Viewport,
    terminals: &[Rect],
    sidebar_pixels: u16,
) -> Vec<Component> {
    let full = Rect {
        x: 0,
        y: 0,
        width: viewport.columns,
        height: viewport.rows,
    };
    let fallback = |surface| {
        vec![Component::Surface {
            rect: full,
            surface,
        }]
    };
    if terminals.is_empty()
        || !viewport.width.is_multiple_of(u32::from(viewport.columns))
        || !viewport.height.is_multiple_of(u32::from(viewport.rows))
    {
        return fallback(surface);
    }
    let cell = [
        viewport.width / u32::from(viewport.columns),
        viewport.height / u32::from(viewport.rows),
    ];
    let left = terminals.iter().map(|r| r.x).min().unwrap();
    let top = terminals.iter().map(|r| r.y).min().unwrap();
    let right = terminals.iter().map(|r| r.x + r.width).max().unwrap();
    let bottom = terminals.iter().map(|r| r.y + r.height).max().unwrap();
    let pixel = |r: Rect| {
        PixelRect::new(
            (u32::from(r.x) * cell[0]) as u16,
            (u32::from(r.y) * cell[1]) as u16,
            (u32::from(r.width) * cell[0]) as u16,
            (u32::from(r.height) * cell[1]) as u16,
        )
    };
    let hole = pixel(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    });
    // Split panes share this background. Any future canvas artwork must keep
    // the original path instead of being silently removed by this optimization.
    if surface.nodes.iter().enumerate().any(|(index, node)| {
        let Some(visible) = intersection(node.rect(), hole) else {
            return false;
        };
        if matches!(node, Primitive::Fill {color, radius:0, ..} if *color==surface.background) {
            return false;
        }
        // The drafting grid is behind an opaque pane background. It remains
        // visible in workspace margins, but needs no raster beneath the canvas.
        !surface.nodes[index + 1..].iter().any(|later| {
            matches!(later, Primitive::Fill {color,radius:0,..} if *color==surface.background)
                && intersection(later.rect(), visible) == Some(visible)
        })
    }) {
        return fallback(surface);
    }
    let sidebar = (u32::from(sidebar_pixels).div_ceil(cell[0]) as u16).min(left);
    // Keep sidebar labels intact vertically and header/footer labels intact
    // horizontally, including when the canvas has drafting-grid margins.
    let regions = [
        Rect {
            x: 0,
            y: 0,
            width: sidebar,
            height: full.height,
        },
        Rect {
            x: sidebar,
            y: 0,
            width: full.width - sidebar,
            height: top,
        },
        Rect {
            x: sidebar,
            y: bottom,
            width: full.width - sidebar,
            height: full.height - bottom,
        },
        Rect {
            x: sidebar,
            y: top,
            width: left - sidebar,
            height: bottom - top,
        },
        Rect {
            x: right,
            y: top,
            width: full.width - right,
            height: bottom - top,
        },
    ];
    let mut components = vec![];
    for rect in regions.into_iter().filter(|r| r.width > 0 && r.height > 0) {
        let Some(tile) = crop(&surface, pixel(rect)) else {
            return fallback(surface);
        };
        components.push(Component::Surface {
            rect,
            surface: tile,
        });
    }
    components
}

/// Transparent popups need only their visible bounds. Global dismissal targets
/// remain in the controller's hit list; the raster carries the visible controls.
pub fn foreground(surface: Surface, viewport: Viewport) -> Component {
    let full = Rect {
        x: 0,
        y: 0,
        width: viewport.columns,
        height: viewport.rows,
    };
    let fallback = |surface| Component::Surface {
        rect: full,
        surface,
    };
    if surface.nodes.is_empty()
        || !viewport.width.is_multiple_of(u32::from(viewport.columns))
        || !viewport.height.is_multiple_of(u32::from(viewport.rows))
    {
        return fallback(surface);
    }
    let cw = viewport.width / u32::from(viewport.columns);
    let ch = viewport.height / u32::from(viewport.rows);
    let left = surface
        .nodes
        .iter()
        .map(|n| u32::from(n.rect().x))
        .min()
        .unwrap()
        / cw;
    let top = surface
        .nodes
        .iter()
        .map(|n| u32::from(n.rect().y))
        .min()
        .unwrap()
        / ch;
    let right = surface
        .nodes
        .iter()
        .map(|n| u32::from(n.rect().x) + u32::from(n.rect().width))
        .max()
        .unwrap()
        .div_ceil(cw);
    let bottom = surface
        .nodes
        .iter()
        .map(|n| u32::from(n.rect().y) + u32::from(n.rect().height))
        .max()
        .unwrap()
        .div_ceil(ch);
    let area = PixelRect::new(
        (left * cw) as u16,
        (top * ch) as u16,
        ((right - left) * cw) as u16,
        ((bottom - top) * ch) as u16,
    );
    match crop(&surface, area) {
        Some(tile) => Component::Surface {
            rect: Rect {
                x: left as u16,
                y: top as u16,
                width: (right - left) as u16,
                height: (bottom - top) as u16,
            },
            surface: tile,
        },
        None => fallback(surface),
    }
}

/// A retained scene for terminal traffic that changes no workspace controls.
#[derive(Default)]
pub struct Cache {
    key: Vec<u8>,
    scene: Option<starkit::terminal_graphics::protocol::Scene>,
    hits: std::cell::Cell<u64>,
    builds: u64,
}
impl Cache {
    pub fn get(&self, key: &[u8]) -> Option<starkit::terminal_graphics::protocol::Scene> {
        if self.key != key {
            return None;
        }
        let scene = self.scene.as_ref()?;
        self.hits.set(self.hits.get() + 1);
        Some(scene.clone())
    }
    pub fn store(&mut self, key: Vec<u8>, scene: &starkit::terminal_graphics::protocol::Scene) {
        self.key = key;
        self.scene = Some(scene.clone());
        self.builds += 1;
    }
    pub fn statistics(&self) -> serde_json::Value {
        let surface_pixels = self.scene.as_ref().map_or(0, |scene| {
            scene
                .components
                .iter()
                .filter_map(|c| match c {
                    Component::Surface { surface, .. } => {
                        Some(u64::from(surface.width) * u64::from(surface.height))
                    }
                    _ => None,
                })
                .sum::<u64>()
        });
        serde_json::json!({"scene_builds":self.builds,"scene_cache_hits":self.hits.get(),"surface_pixels":surface_pixels})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{appearance::Appearance, project_state::Space, shelf};
    use starkit::terminal_graphics::{
        protocol::Scene,
        renderer::{Backend, SceneRenderer},
    };

    fn sample(viewport: Viewport, phase: u16, tiled: bool, legacy_overlay: bool) -> Scene {
        sample_overlay(viewport, phase, tiled, legacy_overlay, false)
    }
    fn sample_overlay(
        viewport: Viewport,
        phase: u16,
        tiled: bool,
        legacy_overlay: bool,
        popup: bool,
    ) -> Scene {
        sample_space(viewport, phase, tiled, legacy_overlay, popup, false)
    }
    fn sample_space(
        viewport: Viewport,
        phase: u16,
        tiled: bool,
        legacy_overlay: bool,
        popup: bool,
        apps: bool,
    ) -> Scene {
        let space = if apps { Space::Apps } else { Space::Terminals };
        let theme = Appearance::load("kanagawa").unwrap();
        let menu = shelf::ContextMenu {
            position: [311, 123],
            title: "Terminal".into(),
            subtitle: "Session controls".into(),
            selected: Some(0),
            items: vec![shelf::MenuItem {
                label: "New terminal".into(),
                hint: "Ctrl+T".into(),
                action: "terminal:new".into(),
            }],
        };
        let layout = shelf::Layout::new(viewport.width as u16, viewport.height as u16, space);
        let model = shelf::Model {
            app_view: apps,
            project: "sample",
            project_label: "Renderer benchmark",
            project_offset: 0,
            project_total: 24,
            sort_hover: false,
            sort_mode_hover: false,
            project_manual: true,
            project_drag: None,
            project_sort_high: true,
            wave_phase: phase,
            reduced_motion: false,
            directory: "/work/render-performance",
            host: "local",
            theme: &theme.name,
            audio_label: "",
            space,
            projects: (0..24)
                .map(|i| shelf::ProjectTab {
                    id: "sample",
                    name: "Rendering project",
                    host: "local",
                    working: usize::from(i % 2 == 0),
                    waiting: 0,
                    running: 1,
                    questions: 0,
                })
                .collect(),
            terminals: vec![(0, "Agent".into()), (1, "Build".into())],
            terminal_status: Default::default(),
            terminal: 0,
            terminal_font_size: "12pt".into(),
            split: false,
            agent_name: "Codex",
            agent_state: "working",
            status: "Project session connected / process retained",
            apps: vec![],
            app_desktop: None,
            connected: apps,
            desktop_host: "local",
            context_menu: popup.then_some(&menu),
            git: None,
            git_open: false,
            git_scroll: 0,
            modal: None,
            transferring: false,
            hover: false,
        };
        let (surface, foreground) = shelf::surfaces(layout, &theme, &model);
        assert!(
            popup || foreground.nodes.is_empty(),
            "Idle chrome must not allocate an empty full-window overlay"
        );
        let mut scene = Scene::from_buffer(
            &starkit::ratatui::buffer::Buffer::empty(starkit::ratatui::layout::Rect::new(
                0,
                0,
                viewport.columns,
                viewport.rows,
            )),
            viewport,
            1,
        );
        scene.background = theme.bg;
        scene.spans.clear();
        let full = Rect {
            x: 0,
            y: 0,
            width: viewport.columns,
            height: viewport.rows,
        };
        let terminal = crate::ui::native_terminal_rect(
            PixelRect::new(0, 0, layout.canvas.width, layout.canvas.height),
            layout.canvas,
            viewport,
        )
        .0;
        scene.components = if tiled {
            chrome(surface, viewport, &[terminal], layout.sidebar.width)
        } else {
            vec![Component::Surface {
                rect: full,
                surface,
            }]
        };
        if !apps {
            scene
                .components
                .push(Component::Terminal { rect: terminal });
        }
        if popup {
            scene.components.push(if tiled {
                super::foreground(foreground, viewport)
            } else {
                Component::Surface {
                    rect: full,
                    surface: foreground,
                }
            });
        }
        if legacy_overlay {
            let mut overlay = Surface::new(
                viewport.width as u16,
                viewport.height as u16,
                "transparent".into(),
            )
            .with_physical_text();
            overlay.fill(PixelRect::new(0, 0, 1, 1), &theme.border, 0);
            scene.components.push(Component::Surface {
                rect: full,
                surface: overlay,
            });
        }
        scene
    }
    #[test]
    fn retained_chrome_matches_monolithic_pixels_and_hit_targets() {
        let mut renderer = SceneRenderer::new(Backend::Cpu).unwrap();
        for viewport in [
            Viewport {
                columns: 100,
                rows: 30,
                width: 800,
                height: 600,
                generation: 1,
            },
            Viewport {
                columns: 264,
                rows: 130,
                width: 1848,
                height: 2080,
                generation: 1,
            },
            Viewport {
                columns: 320,
                rows: 90,
                width: 2560,
                height: 1440,
                generation: 1,
            },
        ] {
            let original = sample(viewport, 41, false, false);
            let tiled = sample(viewport, 41, true, false);
            let expected = renderer.render(&original).unwrap();
            let actual = renderer.render(&tiled).unwrap();
            if expected != actual {
                let root = std::path::Path::new("target/graphical-render-proof");
                std::fs::create_dir_all(root).unwrap();
                expected.save(root.join("expected.png")).unwrap();
                actual.save(root.join("actual.png")).unwrap();
                let changed = expected
                    .pixels()
                    .zip(actual.pixels())
                    .filter(|(a, b)| a != b)
                    .count();
                panic!(
                    "Chrome raster changed at {}x{}: {changed} pixels",
                    viewport.width, viewport.height
                );
            }
            let pixels: u64 = tiled
                .components
                .iter()
                .filter_map(|c| match c {
                    Component::Surface { surface, .. } => {
                        Some(u64::from(surface.width) * u64::from(surface.height))
                    }
                    _ => None,
                })
                .sum();
            if viewport.height > 1000 {
                assert!(pixels * 2 < u64::from(viewport.width) * u64::from(viewport.height));
            }
            let original_surface = match &original.components[0] {
                Component::Surface { surface, .. } => surface,
                _ => unreachable!(),
            };
            for hit in &original_surface.hits {
                let x = hit.rect.x + hit.rect.width / 2;
                let y = hit.rect.y + hit.rect.height / 2;
                assert!(
                    tiled.components.iter().any(|c| {
                        let Component::Surface { rect, surface } = c else {
                            return false;
                        };
                        let px = (u32::from(rect.x) * viewport.width / u32::from(viewport.columns))
                            as u16;
                        let py =
                            (u32::from(rect.y) * viewport.height / u32::from(viewport.rows)) as u16;
                        surface.hits.iter().any(|h| {
                            h.action == hit.action
                                && PixelRect::new(
                                    h.rect.x + px,
                                    h.rect.y + py,
                                    h.rect.width,
                                    h.rect.height,
                                )
                                .contains(x, y)
                        })
                    }),
                    "Lost hit target {}",
                    hit.action
                );
            }
        }
    }
    #[test]
    fn app_chrome_preserves_pixels_without_rasterizing_the_desktop() {
        let viewport = Viewport {
            columns: 288,
            rows: 76,
            width: 2304,
            height: 1444,
            generation: 1,
        };
        let original = sample_space(viewport, 41, false, false, false, true);
        let tiled = sample_space(viewport, 41, true, false, false, true);
        let mut renderer = SceneRenderer::new(Backend::Cpu).unwrap();
        let expected = renderer.render(&original).unwrap();
        let actual = renderer.render(&tiled).unwrap();
        let changes = expected
            .enumerate_pixels()
            .zip(actual.pixels())
            .filter(|((_, _, a), b)| *a != *b)
            .map(|((x, y, a), b)| (x, y, *a, *b))
            .take(3)
            .collect::<Vec<_>>();
        assert!(changes.is_empty(), "Changed pixels: {changes:?}");
        let pixels: u64 = tiled
            .components
            .iter()
            .filter_map(|c| match c {
                Component::Surface { surface, .. } => {
                    Some(u64::from(surface.width) * u64::from(surface.height))
                }
                _ => None,
            })
            .sum();
        let canvas =
            shelf::Layout::new(viewport.width as u16, viewport.height as u16, Space::Apps).canvas;
        let hole = crate::ui::native_terminal_rect(
            PixelRect::new(0, 0, canvas.width, canvas.height),
            canvas,
            viewport,
        )
        .0;
        let excluded = u64::from(hole.width)
            * u64::from(viewport.width / u32::from(viewport.columns))
            * u64::from(hole.height)
            * u64::from(viewport.height / u32::from(viewport.rows));
        assert_eq!(
            pixels + excluded,
            u64::from(viewport.width) * u64::from(viewport.height),
            "Rasterize the chrome around the capped app, excluding its live pixels"
        );
    }
    #[test]
    #[ignore = "Manual App Space chrome comparison; writes target/graphical-render-proof/app-chrome.json"]
    fn app_chrome_benchmark() {
        let viewport = Viewport {
            columns: 288,
            rows: 76,
            width: 2304,
            height: 1444,
            generation: 1,
        };
        let mut result = serde_json::Map::new();
        for (name, tiled) in [("before", false), ("after", true)] {
            let mut renderer = SceneRenderer::new(Backend::Cpu).unwrap();
            let mut times = vec![];
            for i in 0..28 {
                let scene = sample_space(viewport, i * 7, tiled, false, false, true);
                let began = std::time::Instant::now();
                std::hint::black_box(renderer.render(&scene).unwrap());
                if i >= 4 {
                    times.push(began.elapsed().as_secs_f64() * 1000.);
                }
            }
            times.sort_by(f64::total_cmp);
            result.insert(name.into(),serde_json::json!({"median_ms":times[times.len()/2],"p95_ms":times[times.len()*95/100]}));
        }
        let path = std::path::Path::new("target/graphical-render-proof");
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(
            path.join("app-chrome.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        println!("{}", serde_json::Value::Object(result));
    }
    #[test]
    fn popup_raster_matches_full_overlay() {
        let viewport = Viewport {
            columns: 264,
            rows: 130,
            width: 1848,
            height: 2080,
            generation: 1,
        };
        let original = sample_overlay(viewport, 0, false, false, true);
        let tiled = sample_overlay(viewport, 0, true, false, true);
        let mut renderer = SceneRenderer::new(Backend::Cpu).unwrap();
        assert_eq!(
            renderer.render(&original).unwrap(),
            renderer.render(&tiled).unwrap()
        );
        let Component::Surface { surface, .. } = tiled.components.last().unwrap() else {
            panic!("Missing popup")
        };
        assert!(u32::from(surface.width) * u32::from(surface.height) < 100_000);
    }
    #[test]
    fn fractional_geometry_preserves_the_original_surface() {
        let mut viewport = Viewport {
            columns: 100,
            rows: 30,
            width: 801,
            height: 600,
            generation: 1,
        };
        let scene = sample(viewport, 0, true, false);
        assert_eq!(scene.components.len(), 2);
        viewport.width = 800;
        viewport.height = 601;
        assert_eq!(sample(viewport, 0, true, false).components.len(), 2);
    }
    #[test]
    #[ignore = "Manual CPU chrome benchmark; writes target/graphical-render-proof/chrome.json"]
    fn graphical_chrome_benchmark() {
        use std::time::Instant;
        let viewport = Viewport {
            columns: 264,
            rows: 130,
            width: 1848,
            height: 2080,
            generation: 1,
        };
        let mut results = serde_json::Map::new();
        for (name, tiled, overlay) in [("before", false, true), ("after", true, false)] {
            let mut renderer = SceneRenderer::new(Backend::Cpu).unwrap();
            let mut samples = vec![];
            let mut area = 0;
            for i in 0..28 {
                let scene = sample(viewport, i * 7, tiled, overlay);
                area = scene
                    .components
                    .iter()
                    .filter_map(|c| match c {
                        Component::Surface { surface, .. } => {
                            Some(u64::from(surface.width) * u64::from(surface.height))
                        }
                        _ => None,
                    })
                    .sum::<u64>();
                let start = Instant::now();
                let rendered = renderer.render(&scene).unwrap();
                std::hint::black_box(rendered);
                if i >= 4 {
                    samples.push(start.elapsed().as_secs_f64() * 1000.);
                }
            }
            samples.sort_by(f64::total_cmp);
            results.insert(name.into(),serde_json::json!({"median_ms":samples[samples.len()/2],"p95_ms":samples[samples.len()*95/100],"surface_pixels":area,"samples_ms":samples}));
        }
        let root = std::path::Path::new("target/graphical-render-proof");
        std::fs::create_dir_all(root).unwrap();
        let value = serde_json::Value::Object(results);
        std::fs::write(
            root.join("chrome.json"),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
        println!("{value}");
    }
}
