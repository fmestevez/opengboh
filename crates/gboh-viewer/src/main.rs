//! Interactive battlefield viewer.
//!
//! `gboh-viewer <DATA dir> [MAP or SCN file]` opens a window on a battle map
//! (with its units, for a scenario) drawn by the faithful renderer. The battlefield stays 8-bit: it is uploaded as a
//! palette-index texture and coloured on the GPU through a 256-entry palette
//! texture, so the water animation only re-uploads the palette, as the
//! original did.
//!
//! Keys: arrows/WASD or drag to scroll, wheel to scroll, 1/2/3 = zoom in /
//! normal / out, +/- = pixel scale, space = toggle water animation, Esc =
//! quit. With a scenario the battle can be played hot seat (see `play`):
//! click units and hexes, Q/E to turn, R to rally, C to recover, right
//! click to volley, G to try a group move, Enter to finish a leader (M: and
//! try for momentum).

mod play;

use anyhow::{Context, Result, bail};
use gboh_formats::map::Map;
use gboh_formats::pal::Palette;
use gboh_formats::scn::Scenario;
use gboh_formats::units::ART_TITLES;
use gboh_render::battlefield::{self, Scene};
use gboh_render::units::UnitView;
use gboh_render::terrain::Zoom;
use gboh_render::Canvas;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// Time per water animation frame. The original's rate is not known yet.
const WATER_FRAME: Duration = Duration::from_millis(250);
const ZOOMS: [Zoom; 3] = [Zoom::In, Zoom::Normal, Zoom::Out];

const SHADER: &str = r#"
struct View {
    origin: vec2<f32>,
    size: vec2<f32>,
    scale: f32,
};

@group(0) @binding(0) var indices: texture_2d<u32>;
@group(0) @binding(1) var palette: texture_2d<f32>;
@group(0) @binding(2) var<uniform> view: View;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = floor(view.origin + pos.xy / view.scale);
    if (p.x < 0.0 || p.y < 0.0 || p.x >= view.size.x || p.y >= view.size.y) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let i = textureLoad(indices, vec2<i32>(p), 0).r;
    return textureLoad(palette, vec2<i32>(i32(i), 0), 0);
}
"#;

/// Uniform block matching `View` in the shader (padded to 32 bytes).
fn view_uniform(origin: [f32; 2], size: [f32; 2], scale: f32) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, v) in [origin[0], origin[1], size[0], size[1], scale].into_iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

/// One zoom level's battlefield on the GPU.
struct Level {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    palette: wgpu::Texture,
    uniform: wgpu::Buffer,
    levels: Vec<Level>,
}

impl Gpu {
    fn new(window: Arc<Window>, canvases: &[Canvas]) -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .context("no GPU adapter")?;
        let widest = canvases.iter().map(|c| c.width.max(c.height) as u32).max().unwrap_or(0);
        let limits = adapter.limits();
        if widest > limits.max_texture_dimension_2d {
            bail!("battlefield is {widest} px wide, the GPU allows {}", limits.max_texture_dimension_2d);
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))?;

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("surface not supported")?;
        // palette colours are written as-is, so avoid an sRGB conversion
        let caps = surface.get_capabilities(&adapter);
        if let Some(&f) = caps.formats.iter().find(|f| !f.is_srgb()) {
            config.format = f;
        }
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let texture_entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                texture_entry(0, wgpu::TextureSampleType::Uint),
                texture_entry(1, wgpu::TextureSampleType::Float { filterable: false }),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(config.format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let palette = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palette"),
            size: wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let palette_view = palette.create_view(&Default::default());
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let levels = canvases
            .iter()
            .map(|c| {
                let (width, height) = (c.width as u32, c.height as u32);
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("battlefield"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Uint,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    texture.as_image_copy(),
                    &c.pixels,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width), rows_per_image: None },
                    texture.size(),
                );
                let view = texture.create_view(&Default::default());
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&palette_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                    ],
                });
                Level { width, height, texture, bind_group }
            })
            .collect();

        Ok(Self { window, surface, device, queue, config, pipeline, palette, uniform, levels })
    }

    /// Replace a level's battlefield pixels (same size).
    fn update_level(&self, level: usize, canvas: &Canvas) {
        let l = &self.levels[level];
        self.queue.write_texture(
            l.texture.as_image_copy(),
            &canvas.pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(l.width), rows_per_image: None },
            l.texture.size(),
        );
    }

    fn set_palette(&self, pal: &Palette) {
        let rgba: Vec<u8> = pal.colors.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect();
        self.queue.write_texture(
            self.palette.as_image_copy(),
            &rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256 * 4), rows_per_image: None },
            self.palette.size(),
        );
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.config.width = size.width.max(1);
        self.config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    fn draw(&self, level: usize, origin: [f32; 2], scale: f32) {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(_) => return,
        };
        let l = &self.levels[level];
        self.queue.write_buffer(&self.uniform, 0, &view_uniform(origin, [l.width as f32, l.height as f32], scale));
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &l.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        frame.present();
    }
}

struct Viewer {
    title: String,
    canvases: Vec<Canvas>,
    scenes: Vec<Scene>,
    play: Option<play::Play>,
    /// Levels whose canvas is out of date with the battle.
    stale: [bool; 3],
    press: Option<PhysicalPosition<f64>>,
    base_palette: Palette,
    gpu: Option<Gpu>,
    level: usize,
    /// Canvas pixel at the window centre.
    centre: [f32; 2],
    scale: f32,
    water_frame: usize,
    animate: bool,
    next_frame: Instant,
    drag: Option<PhysicalPosition<f64>>,
    cursor: PhysicalPosition<f64>,
}

impl Viewer {
    fn origin(&self) -> [f32; 2] {
        let size = self.gpu.as_ref().map_or(PhysicalSize::new(1, 1), |g| g.window.inner_size());
        [
            self.centre[0] - size.width as f32 / 2.0 / self.scale,
            self.centre[1] - size.height as f32 / 2.0 / self.scale,
        ]
    }

    fn scroll(&mut self, dx: f32, dy: f32) {
        let c = &self.canvases[self.level];
        self.centre[0] = (self.centre[0] + dx / self.scale).clamp(0.0, c.width as f32);
        self.centre[1] = (self.centre[1] + dy / self.scale).clamp(0.0, c.height as f32);
        self.redraw();
    }

    /// Switch zoom level, keeping roughly the same spot in the middle.
    fn set_level(&mut self, level: usize) {
        let (w0, _, f0) = ZOOMS[self.level].block();
        let (w1, _, f1) = ZOOMS[level].block();
        self.centre[0] *= w1 as f32 / w0 as f32;
        self.centre[1] *= f1 as f32 / f0 as f32;
        self.level = level;
        self.refresh();
        self.update_title();
        self.redraw();
    }

    fn update_title(&self) {
        if let Some(g) = &self.gpu {
            let zoom = ["Zoom In", "Normal", "Zoom Out"][self.level];
            let status = self.play.as_ref().map_or(String::new(), |p| format!(" | {}", p.status));
            g.window.set_title(&format!("opengboh: {} ({zoom}, {}x){status}", self.title, self.scale));
        }
    }

    /// Redraw the battle at the current level if it changed.
    fn refresh(&mut self) {
        let Some(p) = &self.play else { return };
        if !self.stale[self.level] {
            return;
        }
        let scene = &self.scenes[self.level];
        let mut canvas = scene.compose(&p.views(), &p.flagged());
        play::light(&mut canvas, scene, &p.lit(), &self.base_palette);
        if let Some(g) = &self.gpu {
            g.update_level(self.level, &canvas);
        }
        self.canvases[self.level] = canvas;
        self.stale[self.level] = false;
    }

    /// The battle changed: redraw.
    fn changed(&mut self) {
        self.stale = [true; 3];
        self.refresh();
        self.update_title();
        self.redraw();
    }

    /// A click at the cursor.
    fn click(&mut self, right: bool) {
        let o = self.origin();
        let (px, py) = (o[0] + self.cursor.x as f32 / self.scale, o[1] + self.cursor.y as f32 / self.scale);
        let Some(hex) = play::pick(&self.scenes[self.level], px as i32, py as i32) else { return };
        if self.play.as_mut().is_some_and(|p| p.click(hex, right)) {
            self.changed();
        }
    }

    fn upload_palette(&self) {
        if let Some(g) = &self.gpu {
            let mut pal = self.base_palette.clone();
            gboh_render::animate_water(&mut pal, self.water_frame);
            g.set_palette(&pal);
        }
    }

    fn redraw(&self) {
        if let Some(g) = &self.gpu {
            g.window.request_redraw();
        }
    }
}

impl ApplicationHandler for Viewer {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        // one battlefield pixel per logical pixel, like the original on a
        // 1x display
        self.scale = window.scale_factor().round().max(1.0) as f32;
        match Gpu::new(window, &self.canvases) {
            Ok(g) => self.gpu = Some(g),
            Err(e) => {
                eprintln!("error: {e:#}");
                event_loop.exit();
                return;
            }
        }
        self.upload_palette();
        self.stale = [true; 3];
        self.refresh();
        self.update_title();
        self.redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        const STEP: f32 = 64.0;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(g) = &mut self.gpu {
                    g.resize(size);
                }
                self.redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Some(g) = &self.gpu {
                    g.draw(self.level, self.origin(), self.scale);
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    Key::Named(NamedKey::ArrowLeft) => self.scroll(-STEP, 0.0),
                    Key::Named(NamedKey::ArrowRight) => self.scroll(STEP, 0.0),
                    Key::Named(NamedKey::ArrowUp) => self.scroll(0.0, -STEP),
                    Key::Named(NamedKey::ArrowDown) => self.scroll(0.0, STEP),
                    Key::Named(NamedKey::Space) => self.animate = !self.animate,
                    Key::Named(NamedKey::Enter) => {
                        if self.play.as_mut().is_some_and(|p| p.finish(false)) {
                            self.changed();
                        }
                    }
                    Key::Character(c) => match c.to_ascii_lowercase().as_str() {
                        "a" => self.scroll(-STEP, 0.0),
                        "d" => self.scroll(STEP, 0.0),
                        "w" => self.scroll(0.0, -STEP),
                        "s" => self.scroll(0.0, STEP),
                        "1" => self.set_level(0),
                        "2" => self.set_level(1),
                        "3" => self.set_level(2),
                        "m" => {
                            if self.play.as_mut().is_some_and(|p| p.finish(true)) {
                                self.changed();
                            }
                        }
                        "g" => {
                            if self.play.as_mut().is_some_and(|p| p.group_move()) {
                                self.changed();
                            }
                        }
                        "r" => {
                            if self.play.as_mut().is_some_and(|p| p.rally()) {
                                self.changed();
                            }
                        }
                        "c" => {
                            if self.play.as_mut().is_some_and(|p| p.recover()) {
                                self.changed();
                            }
                        }
                        "q" | "e" => {
                            if self.play.as_mut().is_some_and(|p| p.turn(c.eq_ignore_ascii_case("q"))) {
                                self.changed();
                            }
                        }
                        "+" | "=" => {
                            self.scale = (self.scale + 1.0).min(8.0);
                            self.update_title();
                            self.redraw();
                        }
                        "-" => {
                            self.scale = (self.scale - 1.0).max(1.0);
                            self.update_title();
                            self.redraw();
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.drag = (state == ElementState::Pressed).then_some(self.cursor);
                if state == ElementState::Pressed {
                    self.press = Some(self.cursor);
                } else if let Some(p) = self.press.take() {
                    // a click, not a drag
                    if (p.x - self.cursor.x).abs() + (p.y - self.cursor.y).abs() < 6.0 {
                        self.click(false);
                    }
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                self.click(true);
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(from) = self.drag {
                    self.scroll((from.x - position.x) as f32, (from.y - position.y) as f32);
                    self.drag = Some(position);
                }
                self.cursor = position;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * STEP, -y * STEP),
                    MouseScrollDelta::PixelDelta(p) => (-p.x as f32, -p.y as f32),
                };
                self.scroll(dx, dy);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.animate {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        let now = Instant::now();
        if now >= self.next_frame {
            self.water_frame = (self.water_frame + 1) % 4;
            self.next_frame = now + WATER_FRAME;
            self.upload_palette();
            self.redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(name)))
}

/// EIF file names in `dir` by title.
fn eif_title_index(dir: &Path) -> std::collections::HashMap<String, String> {
    use std::io::Read;
    let mut heads = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        if !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("eif")) {
            continue;
        }
        let mut head = vec![0u8; 128];
        let Ok(n) = std::fs::File::open(&path).and_then(|mut f| f.read(&mut head)) else { continue };
        head.truncate(n);
        heads.push((path.file_name().unwrap().to_string_lossy().into_owned(), head));
    }
    heads.sort();
    battlefield::eif_titles(heads.iter().map(|(n, h)| (n.as_str(), h.as_slice())))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let Some(data_dir) = args.get(1).map(PathBuf::from) else {
        bail!("usage: gboh-viewer <DATA dir> [MAP or SCN file] [ai]");
    };
    let file = args.get(2).map_or("CHAERONE.SCN", String::as_str);
    let mut load = |name: &str| find_ci(&data_dir, name).and_then(|p| std::fs::read(p).ok());
    // a scenario brings its map and units
    let scenario = if file.to_ascii_lowercase().ends_with(".scn") {
        Some(Scenario::parse(&load(file).with_context(|| format!("{file} not found"))?)?)
    } else {
        None
    };
    let map_file = scenario.as_ref().map_or_else(|| file.to_string(), |s| s.map_file.clone());
    let units = scenario.as_ref().map_or_else(Vec::new, |s| s.units.clone());
    let map_file = map_file.as_str();
    let map = Map::parse(&load(map_file).with_context(|| format!("{map_file} not found"))?)?;
    let flagged = scenario.as_ref().map_or_else(Vec::new, |s| gboh_game::command::opening_flags(s, &map, 1));
    let titles = eif_title_index(&data_dir);
    let mut load_title = |title: &str| titles.get(title).and_then(|n| std::fs::read(data_dir.join(n)).ok());
    let base_palette = {
        let data = load("GAME.PAL").context("GAME.PAL not found")?;
        Palette::parse(&data)?
    };
    let views: Vec<UnitView> = units.iter().enumerate().filter_map(|(i, u)| UnitView::of_scenario(&map, i, u)).collect();
    let types: Vec<usize> = (0..ART_TITLES.len()).collect();
    let scenes = ZOOMS
        .iter()
        .map(|&z| Scene::build(map_file, &map, z, &base_palette, 1, &types, &mut load, &mut load_title))
        .collect::<Result<Vec<_>, _>>()?;
    let canvases: Vec<Canvas> = scenes.iter().map(|s| s.compose(&views, &flagged)).collect();
    // `ai` after the file: the computer plays the visitor army with its
    // first plan
    let ai = match (args.get(3).map(String::as_str), &scenario) {
        (Some("ai"), Some(s)) => {
            let plan = s.visitor_ai.first().context("no AI plan")?;
            let mut read = |name: &str| load(name).map(|d| String::from_utf8_lossy(&d).into_owned());
            let script = gboh_game::ai::Script::load(plan, &mut read).map_err(|e| anyhow::anyhow!("{e}"))?;
            Some((s.visitor.nation, gboh_game::ai::Machine::new(script)))
        }
        _ => None,
    };
    let play = scenario.as_ref().map(|s| play::Play::new(gboh_game::game::Game::new(s, map.clone(), 1), ai));
    let level = 1;
    let centre = [canvases[level].width as f32 / 2.0, canvases[level].height as f32 / 2.0];
    let title = if map.name.is_empty() { map_file.to_string() } else { map.name.clone() };
    let mut viewer = Viewer {
        title,
        canvases,
        scenes,
        play,
        stale: [false; 3],
        press: None,
        base_palette,
        gpu: None,
        level,
        centre,
        scale: 1.0,
        water_frame: 0,
        animate: true,
        next_frame: Instant::now(),
        drag: None,
        cursor: PhysicalPosition::new(0.0, 0.0),
    };
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut viewer)?;
    Ok(())
}
