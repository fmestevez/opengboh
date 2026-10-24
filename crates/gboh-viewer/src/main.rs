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
mod setup;
mod ui;
mod hud;
mod sound;

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

/// Seconds a walking unit takes per hex when the walking data has nothing
/// for it (see `walk_secs`), and per frame:
/// the original ticks its sprites ten times a second (the physical layer's
/// timer callback, `FUN_00500310`, registered at 10 per second).
const WALK_HEX: f32 = 0.2;
const WALK_FRAME: f32 = 0.1;
/// Seconds per volley frame (a sprite tick).
const FIRE_FRAME: f32 = 0.1;
/// Seconds to hold after men fall.
const FALL_PAUSE: f32 = 0.15;
/// Seconds a loose horse or runaway chariot takes per hex (🟡 not
/// measured).
const RUN_HEX: f32 = 0.12;
/// Seconds a shock combat takes on screen: the original spreads its deaths
/// over 7200 ms of sprite time, which runs in real time (ten ticks of 100 ms
/// a second).
const FIGHT_TIME: f32 = 7.2;

/// Time per water animation frame: the palette animator's timer callback
/// runs five times a second (`FUN_004249cd`).
const WATER_FRAME: Duration = Duration::from_millis(200);
const ZOOMS: [Zoom; 3] = [Zoom::In, Zoom::Normal, Zoom::Out];

const SHADER: &str = r#"
struct View {
    origin: vec2<f32>,
    size: vec2<f32>,
    scale: f32,
    overlay: f32,
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
    let outside = p.x < 0.0 || p.y < 0.0 || p.x >= view.size.x || p.y >= view.size.y;
    if (outside) {
        if (view.overlay > 0.5) {
            discard;
        }
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let i = textureLoad(indices, vec2<i32>(p), 0).r;
    // the overlay (toolbar, status bar) is see-through where it is empty
    if (view.overlay > 0.5 && i == 0u) {
        discard;
    }
    return textureLoad(palette, vec2<i32>(i32(i), 0), 0);
}
"#;

/// Uniform block matching `View` in the shader (padded to 32 bytes).
fn view_uniform(origin: [f32; 2], size: [f32; 2], scale: f32, overlay: bool) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, v) in [origin[0], origin[1], size[0], size[1], scale, overlay as u8 as f32].into_iter().enumerate() {
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
    palette_view: wgpu::TextureView,
    uniform: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    levels: Vec<Level>,
    /// The toolbar and status bar, drawn over the battlefield.
    overlay: Option<Level>,
    overlay_uniform: wgpu::Buffer,
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

        let overlay_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay view"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut gpu = Self {
            window,
            surface,
            device,
            queue,
            config,
            pipeline,
            palette,
            palette_view,
            uniform,
            layout,
            levels: Vec::new(),
            overlay: None,
            overlay_uniform,
        };
        gpu.set_levels(canvases);
        Ok(gpu)

    }

    /// New textures for `canvases` (a battle's zoom levels, or a screen).
    fn set_levels(&mut self, canvases: &[Canvas]) {
        self.levels = canvases
            .iter()
            .map(|c| {
                let (width, height) = (c.width as u32, c.height as u32);
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("battlefield"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Uint,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                self.queue.write_texture(
                    texture.as_image_copy(),
                    &c.pixels,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width), rows_per_image: None },
                    texture.size(),
                );
                let view = texture.create_view(&Default::default());
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.palette_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: self.uniform.as_entire_binding() },
                    ],
                });
                Level { width, height, texture, bind_group }
            })
            .collect();
    }

    /// The overlay to draw over the battlefield, if any.
    fn set_overlay(&mut self, canvas: Option<&Canvas>) {
        self.overlay = canvas.map(|c| {
            let (width, height) = (c.width as u32, c.height as u32);
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("overlay"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                texture.as_image_copy(),
                &c.pixels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width), rows_per_image: None },
                texture.size(),
            );
            let view = texture.create_view(&Default::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.palette_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.overlay_uniform.as_entire_binding() },
                ],
            });
            Level { width, height, texture, bind_group }
        });
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
        self.queue.write_buffer(&self.uniform, 0, &view_uniform(origin, [l.width as f32, l.height as f32], scale, false));
        if let Some(o) = &self.overlay {
            self.queue.write_buffer(&self.overlay_uniform, 0, &view_uniform([0.0, 0.0], [o.width as f32, o.height as f32], scale, true));
        }
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
            if let Some(o) = &self.overlay {
                pass.set_bind_group(0, &o.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        frame.present();
    }
}

struct Viewer {
    /// The game's `DATA` directory.
    data_dir: PathBuf,
    /// The setup screens, while a battle is being chosen.
    setup: Option<setup::Setup>,
    /// Exit at the next chance (the start screen's Exit).
    quit: bool,
    /// The toolbar and status bar.
    hud: hud::Hud,
    /// Whether the overview map shows.
    show_overview: bool,
    /// The unit details window, while open.
    details: Option<hud::Details>,
    /// The combat results, while shown: the battle picked, and the one whose
    /// details are open.
    results: Option<(Option<usize>, Option<usize>)>,
    /// The game overview, while shown (at the end of a turn).
    game_overview: bool,
    /// The army details over it: which army and the first line shown.
    army: Option<(usize, usize)>,
    /// The end of game dialog, while shown; and whether it has been.
    ending: bool,
    ended: bool,
    /// A dialog popped up during play, and what it is for.
    plain: Option<(hud::Plain, Asked)>,
    /// "Never show this again" ticked on the missile results, and "Don't
    /// show this again" on the finish leader reminder.
    quiet_missiles: bool,
    quiet_finish: bool,
    title: String,
    /// The battle's map file, to build its battlefield again.
    map_file: String,
    canvases: Vec<Canvas>,
    scenes: Vec<Scene>,
    play: Option<play::Play>,
    sound: sound::Sound,
    /// Levels whose canvas is out of date with the battle.
    stale: [bool; 3],
    /// What is being animated: when it started, seconds per step (a hex or
    /// a volley frame) and in all; and what waits its turn.
    playing: Option<(play::Anim, Instant, f32, f32)>,
    queued: std::collections::VecDeque<play::Anim>,
    /// Loose horses and runaway chariots on their way, since when; and
    /// which of the playing fight's have set off.
    runners: Vec<(gboh_render::runaways::Runaway, play::RunPath, Instant)>,
    loosed: std::collections::HashSet<(usize, usize)>,
    /// Demo: when the next computer leader may play.
    next_step: Instant,
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
        self.update_hud();
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
        self.update_hud();
        self.redraw();
    }

    fn update_title(&self) {
        if let (Some(g), true) = (&self.gpu, self.setup.is_some()) {
            g.window.set_title(&format!("opengboh: {}", self.title));
            return;
        }
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
        let pending: Vec<_> = self.queued.iter().cloned().collect();
        let walking = match &self.playing {
            Some((play::Anim::Walk { unit, path, running }, start, per_hex, _)) => {
                let t = start.elapsed().as_secs_f32() / per_hex;
                let i = (t as usize).min(path.len().saturating_sub(2));
                let frac = (t - i as f32).clamp(0.0, 1.0);
                let frame = (start.elapsed().as_secs_f32() / WALK_FRAME) as usize;
                Some((*unit, path[i], path[i + 1], frac, frame, *running))
            }
            _ => None,
        };
        let mut views = p.views_walking(scene, walking, &pending);
        let flights = match &self.playing {
            Some((play::Anim::Volley { shooter, target, missile, .. }, start, per_frame, _)) => {
                let frame = (start.elapsed().as_secs_f32() / per_frame) as usize;
                p.volley(scene, &mut views, *shooter, *target, *missile, frame)
            }
            Some((play::Anim::Drop { unit, .. }, start, per_frame, _)) => {
                p.dropping(&mut views, *unit, (start.elapsed().as_secs_f32() / per_frame) as usize);
                Vec::new()
            }
            _ => Vec::new(),
        };
        let mut corpses = std::borrow::Cow::Borrowed(&p.corpses);
        if let Some((fight @ play::Anim::Fight { .. }, start, per_ms, _)) = &self.playing {
            let down = p.fight(&mut views, fight, (start.elapsed().as_secs_f32() / per_ms) as u32);
            corpses.to_mut().extend(down);
        }
        let runners: Vec<_> = self
            .runners
            .iter()
            .map(|(kind, path, since)| {
                let t = since.elapsed().as_secs_f32();
                (*kind, path.as_slice(), t / RUN_HEX, (t / WALK_FRAME) as usize)
            })
            .collect();
        let mut canvas = scene.compose_all(&views, &p.flagged(), &corpses, &flights, &runners);
        play::light(&mut canvas, scene, &p.lit(), &self.base_palette);
        if let Some(g) = &self.gpu {
            g.update_level(self.level, &canvas);
        }
        self.canvases[self.level] = canvas;
        self.stale[self.level] = false;
    }

    /// Set off the playing fight's loose horses and chariots whose men have
    /// fallen (all of them once it is over).
    fn loose_runaways(&mut self) {
        let Some((play::Anim::Fight { falls, .. }, start, per_ms, total)) = &self.playing else { return };
        let ms = (start.elapsed().as_secs_f32() / per_ms) as u32;
        let ending = start.elapsed().as_secs_f32() >= *total;
        for (fi, f) in falls.iter().enumerate() {
            for (ri, (k, kind, path)) in f.runaways.iter().enumerate() {
                if (ending || f.falls_at(*k) <= ms) && self.loosed.insert((fi, ri)) {
                    self.runners.push((*kind, path.clone(), Instant::now()));
                }
            }
        }
    }

    /// Seconds per hex for `unit` walking `path` (running if routed): the
    /// walking data's step times for its kind (the slowest man's, averaged
    /// over the path), scaled for routed units by the original's factor per
    /// class (the table at `0x60f8e0`: foot and phalanx 0.67, horse 0.9,
    /// elephants 0.5, others 1).
    fn walk_secs(&self, unit: usize, path: &[gboh_game::hex::Hex], running: bool) -> Option<f32> {
        use gboh_render::units::Formation;
        let o = &self.play.as_ref()?.game.battle.units[unit];
        let formation = Formation::of_type(o.type_id);
        let scene = &self.scenes[self.level];
        let facing = scene.screen_facing(o.facing);
        let steps: Vec<i32> = path
            .windows(2)
            .filter_map(|w| {
                let d = gboh_game::ai_host::direction(w[0], w[1]);
                let dir = scene.screen_facing(d as u8);
                // running men face the way they go
                let (a, b) = ((w[0].x.max(0) as usize, w[0].y.max(0) as usize), (w[1].x.max(0) as usize, w[1].y.max(0) as usize));
                scene.step_ms_between(o.type_id, if running { dir } else { facing }, dir, a, b)
            })
            .collect();
        if steps.is_empty() {
            return None;
        }
        let ms = steps.iter().sum::<i32>() as f32 / steps.len() as f32;
        let factor = match formation {
            Formation::Infantry | Formation::Phalanx => 0.67,
            Formation::Cavalry => 0.9,
            Formation::Elephants => 0.5,
            _ => 1.0,
        };
        Some(ms / 1000.0 * if running { factor } else { 1.0 })
    }

    /// Start the next queued animation: quicker the more are waiting, so
    /// the computer's turns do not drag.
    fn next_anim(&mut self) -> Option<(play::Anim, Instant, f32, f32)> {
        let hurry = 1.0 + self.queued.len() as f32 / 4.0;
        let anim = self.queued.pop_front()?;
        let (step, total) = match &anim {
            play::Anim::Walk { unit, path, running } => {
                let per_hex = self.walk_secs(*unit, path, *running).unwrap_or(WALK_HEX) / hurry;
                (per_hex, per_hex * (path.len() - 1) as f32)
            }
            play::Anim::Drop { unit, .. } => {
                let o = &self.play.as_ref()?.game.battle.units[*unit];
                let facing = self.scenes[self.level].screen_facing(o.facing);
                let frames = self.scenes[self.level].unit_sprites(o.type_id).map_or(0, |a| a.drop_frames(facing));
                let per_frame = WALK_FRAME / hurry.min(2.0);
                (per_frame, per_frame * frames as f32)
            }
            play::Anim::Fight { sound, .. } => {
                self.sound.play(*sound);
                let per_ms = FIGHT_TIME / gboh_render::units::FIGHT_MS as f32 / hurry.min(2.0);
                (per_ms, per_ms * (gboh_render::units::FIGHT_MS + 400) as f32)
            }
            play::Anim::Fall(_) => {
                // the men drop at once; a short pause lets it be seen
                self.play.as_mut()?.land(&anim);
                (0.0, FALL_PAUSE / hurry)
            }
            play::Anim::Volley { shooter, target, sound, .. } => {
                self.sound.play(*sound);
                let scene = &self.scenes[self.level];
                let units = &self.play.as_ref()?.game.battle.units;
                let at = |u: usize| {
                    let h = units[u].hex;
                    gboh_render::objects::anchor_at(&scene.map, &scene.layout, (h.x.max(0) as usize, h.y.max(0) as usize), (0, 0))
                };
                let frames = play::Anim::volley_frames(scene, at(*shooter), at(*target)) + 4;
                let per_frame = FIRE_FRAME / hurry.min(2.0);
                (per_frame, per_frame * frames as f32)
            }
        };
        Some((anim, Instant::now(), step, total))
    }

    /// The battle changed: play its sounds, start any walk, and redraw.
    fn changed(&mut self) {
        if let Some(p) = &mut self.play {
            p.take_log();
            for e in p.events.drain(..) {
                self.sound.play(e);
            }
            self.queued.extend(p.anims.drain(..));
            if self.playing.is_none() {
                self.playing = self.next_anim();
            }
        }
        self.stale = [true; 3];
        self.refresh();
        self.next_notice();
        self.update_title();
        self.update_hud();
        self.redraw();
    }

    /// A click at the cursor.
    fn click(&mut self, right: bool) {
        let o = self.origin();
        let (px, py) = (o[0] + self.cursor.x as f32 / self.scale, o[1] + self.cursor.y as f32 / self.scale);
        if let Some(setup) = &mut self.setup {
            match setup.click(px as i32, py as i32) {
                setup::Outcome::Nothing => {}
                setup::Outcome::Redraw => self.show_setup(),
                setup::Outcome::Quit => self.quit = true,
                setup::Outcome::Play { scenario, computer, levels } => {
                    if let Err(e) = self.start_battle(scenario, computer, levels) {
                        eprintln!("error: {e:#}");
                    }
                }
            }
            return;
        }
        // the toolbar and status bar take clicks over them
        let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
        let (w, h) = self.window_px();
        // a dialog popped up during play is modal: its buttons and check
        // boxes; the finish reminder also closes on a click off it
        if let Some((d, asked)) = self.plain.clone() {
            let at = self.hud.plain_at(&d, w, h, cx, cy);
            match (asked, at) {
                (Asked::Momentum, Some(1 | 2)) => {
                    self.plain = None;
                    if self.play.as_mut().is_some_and(|p| p.finish(at == Some(1))) {
                        self.changed();
                    }
                }
                (Asked::Notice(play::Notice::NoOrders), Some(101)) => {
                    self.quiet_finish = d.checked.contains(&17);
                    self.plain = None;
                    self.finish_leader();
                }
                (Asked::Notice(play::Notice::NoOrders), None) => {
                    self.quiet_finish = d.checked.contains(&17);
                    self.plain = None;
                }
                (_, Some(id @ (17 | 201))) => {
                    if let Some((d, _)) = &mut self.plain {
                        if let Some(k) = d.checked.iter().position(|&c| c == id) {
                            d.checked.remove(k);
                        } else {
                            d.checked.push(id);
                        }
                    }
                }
                (Asked::Notice(_), Some(1)) => {
                    self.quiet_missiles |= d.checked.contains(&201);
                    self.plain = None;
                }
                _ => return,
            }
            self.next_notice();
            self.update_hud();
            self.redraw();
            return;
        }
        // the army details are modal: only Close does anything
        if self.army.is_some() {
            if self.hud.army_close_at(w, h, cx, cy) {
                self.army = None;
                self.update_hud();
                self.redraw();
            }
            return;
        }
        // so is the game overview: OK, or an army's details
        if self.game_overview {
            match self.hud.game_overview_at(w, h, cx, cy) {
                Some(1) => {
                    self.game_overview = false;
                    if let Some(p) = &mut self.play {
                        p.turn_ended = false;
                    }
                }
                Some(102) => self.army = Some((0, 0)),
                Some(101) => self.army = Some((1, 0)),
                _ => return,
            }
            self.update_hud();
            self.redraw();
            return;
        }
        // and the end of the battle: its overview, the field, or back to
        // the start screen
        if let Some(e) = self.ending.then(|| self.play.as_ref().and_then(|p| p.ending())).flatten() {
            match self.hud.ending_at(e, w, h, cx, cy) {
                Some(101) => self.game_overview = true,
                Some(102) => self.ending = false,
                Some(1) => {
                    self.back_to_start();
                    return;
                }
                _ => return,
            }
            self.update_hud();
            self.redraw();
            return;
        }
        // the combat results are modal: pick a battle (again to open it),
        // or close
        if let Some((selected, open)) = self.results {
            let shown = self.update_results_view();
            match shown.and_then(|r| self.hud.results_at(&r, w, h, cx, cy)) {
                Some(hud::ResultsClick::Battle(k)) => {
                    if selected == Some(k) {
                        self.results = Some((selected, Some(k)));
                    } else {
                        self.results = Some((Some(k), open));
                        // the map centres on that battle
                        if let Some(c) = self.play.as_ref().and_then(|p| p.combats.get(k)) {
                            let scene = &self.scenes[self.level];
                            let (x, y) = gboh_render::objects::anchor_at(&scene.map, &scene.layout, (c.hex.x.max(0) as usize, c.hex.y.max(0) as usize), (0, 0));
                            self.centre = [x as f32, y as f32];
                        }
                    }
                }
                Some(hud::ResultsClick::CloseDetails) => self.results = Some((selected, None)),
                Some(hud::ResultsClick::Close) => {
                    self.results = None;
                    if let Some(p) = &mut self.play {
                        p.combats.clear();
                    }
                }
                None => return,
            }
            self.update_hud();
            self.redraw();
            return;
        }
        // the unit details window is modal: only its OK does anything
        if self.details.is_some() {
            if self.hud.details_ok_at(w, h, cx, cy) {
                self.details = None;
                self.update_hud();
                self.redraw();
            }
            return;
        }
        if self.play.is_some() && cy < hud::TOOLBAR_HEIGHT {
            if let Some(t) = self.hud.tool_at(cx, cy) {
                self.tool(t);
            }
            return;
        }
        if self.play.is_some() && cy >= h as i32 - hud::STATUS_HEIGHT {
            return;
        }
        // a click on the overview map brings that part into view
        if let Some(battle) = self.overview().map(|o| o.battle)
            && let Some((fx, fy)) = self.hud.overview_at(battle, self.window_px().0, cx, cy)
        {
            let c = &self.canvases[self.level];
            self.centre = [fx * c.width as f32, fy * c.height as f32];
            self.update_hud();
            self.redraw();
            return;
        }
        let Some(hex) = play::pick(&self.scenes[self.level], px as i32, py as i32) else { return };
        if self.play.as_mut().is_some_and(|p| p.click(hex, right)) {
            self.changed();
        }
    }

    /// The window's size in battlefield pixels.
    fn window_px(&self) -> (usize, usize) {
        let size = self.gpu.as_ref().map_or(PhysicalSize::new(1, 1), |g| g.window.inner_size());
        ((size.width as f32 / self.scale).ceil() as usize, (size.height as f32 / self.scale).ceil() as usize)
    }

    /// Redraw the toolbar and status bar (none over the setup screens).
    fn update_hud(&mut self) {
        let (w, h) = self.window_px();
        let canvas = match (&self.setup, &self.play) {
            (None, Some(p)) => {
                let enabled = |t: hud::Tool| {
                    use hud::Tool::*;
                    matches!(
                        t,
                        FinishLeader
                            | CenterOnLeader
                            | RotateLeft
                            | RotateRight
                            | RestoreCohesion
                            | Rally
                            | Overview
                            | HexGrid
                            | ReverseView
                            | ZoomIn
                            | ZoomNormal
                            | ZoomOut
                    ) || (t == UnitDetails && p.selected().is_some())
                        || (t == MissileVolley && p.can_fire())
                };
                let turn = format!("Turn {}", p.game.activation.turn + 1);
                let zoom = ["Zoom In", "Normal", "Zoom Out"][self.level.min(2)];
                let overview = self.overview();
                let results = self.results.map(|(selected, open)| {
                    let row = |r: &play::CombatRow| {
                        let o = &p.game.battle.units[r.unit];
                        hud::ResultRow {
                            name: o.name.clone(),
                            current: r.current,
                            total: o.hits,
                            to_rout: o.tq as i32 - o.hits,
                            routed: r.routed,
                            eliminated: r.eliminated,
                        }
                    };
                    hud::Results {
                        battles: p.combats.len(),
                        selected,
                        open: open.and_then(|k| p.combats.get(k)).map(|c| (c.attackers.iter().map(row).collect(), c.defenders.iter().map(row).collect())),
                    }
                });
                let army = self.army.map(|(side, scroll)| Self::army_details(&p.game, side, scroll));
                let game_overview = self.game_overview.then(|| hud::GameOverview {
                    nations: [p.game.armies[0].0, p.game.armies[1].0],
                    levels: [p.game.armies[0].1, p.game.armies[1].1],
                    lost: [p.game.rout_points(0), p.game.rout_points(1)],
                });
                Some(self.hud.canvas(
                    w,
                    h,
                    enabled,
                    [&p.status, &turn, zoom],
                    overview.as_ref(),
                    self.details.as_ref(),
                    results.as_ref(),
                    game_overview.as_ref(),
                    &p.commentary,
                    army.as_ref(),
                    self.ending.then(|| p.ending()).flatten(),
                    self.plain.as_ref().map(|(d, _)| d),
                ))
            }
            _ => None,
        };
        if let Some(g) = &mut self.gpu {
            g.set_overlay(canvas.as_ref());
        }
    }

    /// The combat results' battle count and state, for hit-testing.
    fn update_results_view(&self) -> Option<hud::Results> {
        let (selected, open) = self.results?;
        let p = self.play.as_ref()?;
        Some(hud::Results { battles: p.combats.len(), selected, open: open.map(|_| (Vec::new(), Vec::new())) })
    }

    /// What the overview map shows now, if it shows.
    fn overview(&self) -> Option<hud::Overview<'static>> {
        let p = self.play.as_ref().filter(|_| self.show_overview)?;
        // the overview maps by battle, in the scenario table's order
        const NAMES: [&str; 11] =
            ["Chaeronea", "Lyginus", "Pelium", "Granicus", "Issus", "Gaugamela", "Samarkand", "Jaxartes", "Arigaeum", "Hydaspes", "Custom"];
        let battle = NAMES[p.game.battle_index?];
        let scene = &self.scenes[self.level];
        let c = &self.canvases[self.level];
        let (cw, ch) = (c.width as f32, c.height as f32);
        let home = p.game.armies[0].0;
        let dots = p
            .game
            .battle
            .units
            .iter()
            .filter(|u| u.on_field() && u.hex.x >= 0 && u.hex.y >= 0)
            .map(|u| {
                let (x, y) = gboh_render::objects::anchor_at(&scene.map, &scene.layout, (u.hex.x as usize, u.hex.y as usize), (0, 0));
                // the home army red, the visitor blue (the palette's
                // Windows colours)
                (x as f32 / cw, y as f32 / ch, if u.nation == home { 249 } else { 252 })
            })
            .collect();
        let o = self.origin();
        let (w, h) = self.window_px();
        let view = [o[0] / cw, o[1] / ch, (o[0] + w as f32) / cw, (o[1] + h as f32) / ch];
        Some(hud::Overview { battle, dots, view, reversed: scene.layout.reversed })
    }

    /// What the army details show for army `side` (0 home): its units in
    /// the scenario's order, then its leaders.
    fn army_details(game: &gboh_game::game::Game, side: usize, scroll: usize) -> hud::ArmyDetails {
        use gboh_game::battle::UnitState;
        let (nation, level) = game.armies[side];
        let units: Vec<_> = game.battle.units.iter().filter(|u| u.side == nation).collect();
        let lost = |u: &gboh_game::battle::Unit| {
            (u.state == UnitState::Gone).then(|| gboh_game::combat::rout_points(u.type_id, u.tq, u.leader.map_or(0, |r| r.initiative)))
        };
        let rows = units
            .iter()
            .filter(|u| u.leader.is_none())
            .map(|u| hud::ArmyRow { name: u.name.clone(), tq: Some(u.tq), hits: Some(u.hits), lost: lost(u) })
            .chain(units.iter().filter(|u| u.leader.is_some()).map(|u| hud::ArmyRow { name: u.name.clone(), tq: None, hits: None, lost: lost(u) }))
            .collect();
        hud::ArmyDetails {
            nation,
            level,
            // 🟡 what the original counts here is not traced
            routing_tq: units.iter().filter(|u| u.state == UnitState::Routed).map(|u| u.tq).sum(),
            eliminated: game.rout_points(side),
            rows,
            scroll,
        }
    }

    /// Turn the battlefield half round (or back), keeping the same spot in
    /// the middle of the window, as the original's Reverse View does.
    fn reverse(&mut self) {
        let Some(p) = &mut self.play else { return };
        let reversed = !self.scenes[self.level].layout.reversed;
        let grid = self.scenes[self.level].grid_on;
        let scenes = match build_scenes(&self.data_dir, &self.map_file, &p.game.map, reversed) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("reverse view: {e:#}");
                return;
            }
        };
        p.set_reversed(reversed);
        self.queued.iter_mut().for_each(play::Anim::turn);
        if let Some((a, ..)) = &mut self.playing {
            a.turn();
        }
        for (_, path, _) in self.runners.iter_mut() {
            for (_, d) in path.iter_mut() {
                *d = (*d + 3) % 8 + 1;
            }
        }
        let c = &self.canvases[self.level];
        let (cw, ch) = (c.width as f32, c.height as f32);
        self.centre = [cw - self.centre[0], ch - self.centre[1]];
        self.scenes = scenes;
        for s in self.scenes.iter_mut() {
            s.grid_on = grid;
        }
        self.stale = [true; 3];
        self.refresh();
    }

    /// A toolbar button pressed.
    fn tool(&mut self, tool: hud::Tool) {
        use hud::Tool::*;
        let Some(p) = &mut self.play else { return };
        let changed = match tool {
            FinishLeader => {
                self.finish_leader();
                false
            }
            RotateLeft => p.turn(true),
            RotateRight => p.turn(false),
            RestoreCohesion => p.recover(),
            Rally => p.rally(),
            CenterOnLeader => {
                if let Some(l) = p.leader() {
                    let h = p.game.battle.units[l].hex;
                    let scene = &self.scenes[self.level];
                    let (x, y) = gboh_render::objects::anchor_at(&scene.map, &scene.layout, (h.x.max(0) as usize, h.y.max(0) as usize), (0, 0));
                    self.centre = [x as f32, y as f32];
                    self.redraw();
                }
                false
            }
            Overview => {
                self.show_overview = !self.show_overview;
                false
            }
            MissileVolley => {
                p.arm_volley();
                false
            }
            ReverseView => {
                self.reverse();
                false
            }
            HexGrid => {
                let on = !self.scenes[self.level].grid_on;
                for s in self.scenes.iter_mut() {
                    s.grid_on = on;
                }
                false
            }
            UnitDetails => {
                if let Some(u) = p.selected() {
                    let o = &p.game.battle.units[u];
                    use gboh_game::missile::Weapon::*;
                    self.details = Some(hud::Details {
                        name: o.name.clone(),
                        used: o.done || o.fired || o.mp < o.max_mp,
                        routing: o.state == gboh_game::battle::UnitState::Routed,
                        tq: o.tq,
                        hits: o.hits,
                        mp_allowance: o.max_mp,
                        mp_left: o.mp,
                        size: o.size,
                        missile: o.missile.map(|(w, left)| {
                            let kind = match w {
                                Bow | MountedBow => 64,
                                Sling => 65,
                                Javelin | MountedJavelin => 66,
                                Bolt => 67,
                            };
                            (kind, w.range(), w.supply(), left)
                        }),
                    });
                }
                false
            }
            ZoomIn => {
                self.set_level(0);
                false
            }
            ZoomNormal => {
                self.set_level(1);
                false
            }
            ZoomOut => {
                self.set_level(2);
                false
            }
            _ => false,
        };
        if changed {
            self.changed();
        }
        self.update_hud();
        self.redraw();
    }

    /// Show the setup screen as it now is.
    /// Finish the active leader: first asking whether he tries for
    /// momentum, if he may (the original's `MOMENTUMDLG`).
    fn finish_leader(&mut self) {
        let Some(p) = &mut self.play else { return };
        if p.over {
            return;
        }
        if p.game.can_attempt_momentum() {
            self.plain = Some((hud::Plain { name: "MOMENTUMDLG", texts: Vec::new(), checked: Vec::new() }, Asked::Momentum));
            self.update_hud();
            self.redraw();
        } else if p.finish(false) {
            self.changed();
        }
    }

    /// Pop up the next thing to tell the player, if no dialog is up: the
    /// original's momentum, rally and missile results and its finish leader
    /// reminder, in the viewer's words (🟡 the original's are in its data).
    fn next_notice(&mut self) {
        if self.plain.is_some() {
            return;
        }
        let Some(p) = &mut self.play else { return };
        while !p.notices.is_empty() {
            let notice = p.notices.remove(0);
            let name = |u: usize| p.game.battle.units[u].name.clone();
            let plain = |name: &'static str, texts: Vec<(u16, String)>| Some(hud::Plain { name, texts, checked: Vec::new() });
            let shown = match &notice {
                play::Notice::Momentum { leader, won } => plain(
                    "INITIATIVERESULTS",
                    vec![
                        (101, format!("{} tried for momentum and {}", name(*leader), if *won { "goes again!" } else { "failed." })),
                        (102, String::new()),
                    ],
                ),
                play::Notice::Rally { unit, hits } => plain(
                    "RALLYDLG",
                    vec![
                        (101, format!("{} tries to rally.", name(*unit))),
                        (
                            102,
                            match hits {
                                Some(h) => format!("It rallies with {h} cohesion hit{}.", if *h == 1 { "" } else { "s" }),
                                None => "It fails and leaves the battle.".to_string(),
                            },
                        ),
                        (103, String::new()),
                    ],
                ),
                play::Notice::Volley { .. } if self.quiet_missiles => None,
                play::Notice::Volley { target, hits, routed } => plain(
                    "MISSILERESULTS",
                    vec![
                        (101, format!("{} suffers {hits} cohesion hit{}", name(*target), if *hits == 1 { "" } else { "s" })),
                        (102, if *routed { "and is routing!".to_string() } else { "from the volley.".to_string() }),
                    ],
                ),
                play::Notice::NoOrders if self.quiet_finish || p.demo => None,
                play::Notice::NoOrders => plain("LEADERALMOSTFINISHED", Vec::new()),
            };
            if let Some(d) = shown {
                self.plain = Some((d, Asked::Notice(notice)));
                return;
            }
        }
    }

    /// Leave the battle for the start screen.
    fn back_to_start(&mut self) {
        self.play = None;
        self.scenes.clear();
        (self.ending, self.ended, self.game_overview, self.army, self.results, self.details) = (false, false, false, None, None, None);
        self.queued.clear();
        self.playing = None;
        self.plain = None;
        self.runners.clear();
        self.title = String::from("The Great Battles of Alexander");
        self.setup = Some(setup::Setup::new(&self.data_dir));
        self.show_setup();
        self.update_title();
        self.update_hud();
    }

    fn show_setup(&mut self) {
        let Some(setup) = &self.setup else { return };
        let canvas = setup.canvas();
        self.centre = [canvas.width as f32 / 2.0, canvas.height as f32 / 2.0];
        self.canvases = vec![canvas];
        self.level = 0;
        if let Some(g) = &mut self.gpu {
            g.set_levels(&self.canvases);
        }
        self.redraw();
    }

    /// Leave the setup screens for `scenario`, the computer playing the
    /// sides marked (home, visitor), with the armies' rout levels.
    fn start_battle(&mut self, scenario: &str, computer: [bool; 2], levels: [u16; 2]) -> Result<()> {
        let mut b = load_battle(&self.data_dir, scenario, computer)?;
        if let Some(p) = &mut b.play {
            p.game.armies[0].1 = levels[0];
            p.game.armies[1].1 = levels[1];
        }
        self.setup = None;
        self.title = b.title;
        self.map_file = b.map_file;
        self.scenes = b.scenes;
        self.canvases = b.canvases;
        self.play = b.play;
        self.level = 1;
        self.centre = [self.canvases[1].width as f32 / 2.0, self.canvases[1].height as f32 / 2.0];
        if let Some(g) = &mut self.gpu {
            g.set_levels(&self.canvases);
        }
        if let Some(p) = &mut self.play {
            for e in p.events.drain(..) {
                self.sound.play(e);
            }
        }
        self.stale = [true; 3];
        self.refresh();
        self.update_title();
        self.update_hud();
        self.redraw();
        Ok(())
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
        if let Some(p) = &mut self.play {
            for e in p.events.drain(..) {
                self.sound.play(e);
            }
        }
        self.stale = [true; 3];
        self.refresh();
        self.update_title();
        self.update_hud();
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
                self.update_hud();
                self.redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Some(g) = &self.gpu {
                    g.draw(self.level, self.origin(), self.scale);
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && self.setup.is_some() => {
                if event.logical_key.as_ref() == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
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
                    Key::Named(NamedKey::Enter) if self.plain.is_none() => self.finish_leader(),
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
            WindowEvent::MouseWheel { delta, .. } if self.army.is_some() => {
                // the army details' list scrolls a line at a time
                let down = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y < 0.0,
                    MouseScrollDelta::PixelDelta(p) => p.y < 0.0,
                };
                if let (Some((side, scroll)), Some(p)) = (self.army, &self.play) {
                    let lines = Self::army_details(&p.game, side, 0).rows.len();
                    let last = lines.saturating_sub(hud::ARMY_LINES);
                    self.army = Some((side, if down { (scroll + 1).min(last) } else { scroll.saturating_sub(1) }));
                    self.update_hud();
                    self.redraw();
                }
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
        if self.quit {
            event_loop.exit();
            return;
        }
        // once the fighting has been shown, the combat results come up
        if self.results.is_none()
            && self.playing.is_none()
            && self.queued.is_empty()
            && self.play.as_ref().is_some_and(|p| !p.combats.is_empty() && !p.demo)
        {
            self.results = Some((None, None));
            self.update_hud();
            self.redraw();
        }
        // once the last moves have been shown, the end of the battle
        if !self.ended
            && self.results.is_none()
            && self.playing.is_none()
            && self.queued.is_empty()
            && self.play.as_ref().is_some_and(|p| p.over && p.combats.is_empty())
        {
            self.ended = true;
            self.ending = true;
            self.update_hud();
            self.redraw();
        }
        // and after them, at the end of a turn, the game overview
        if !self.game_overview
            && self.results.is_none()
            && self.playing.is_none()
            && self.queued.is_empty()
            && self.play.as_ref().is_some_and(|p| p.turn_ended && p.combats.is_empty() && !p.demo)
        {
            self.game_overview = true;
            self.update_hud();
            self.redraw();
        }
        // a walking unit: redraw until it arrives
        // demo: when the animations are done, play the next computer leader
        if self.playing.is_none()
            && self.queued.is_empty()
            && self.play.as_ref().is_some_and(|p| p.demo)
            && self.next_step <= Instant::now()
        {
            if self.play.as_mut().is_some_and(|p| p.step_computer()) {
                self.changed();
            }
            self.next_step = Instant::now() + std::time::Duration::from_millis(400);
        }
        // loose horses and runaway chariots run on until off the map
        self.runners.retain(|(_, path, since)| since.elapsed().as_secs_f32() / RUN_HEX < path.len() as f32);
        if self.playing.is_some() || !self.runners.is_empty() {
            self.loose_runaways();
            let done = self.playing.as_ref().is_some_and(|(_, start, _, total)| start.elapsed().as_secs_f32() >= *total);
            if done {
                // a fight's men are down for good; then the next animation
                if let (Some((anim @ (play::Anim::Fight { .. } | play::Anim::Drop { .. } | play::Anim::Volley { .. }), ..)), Some(p)) =
                    (self.playing.take(), self.play.as_mut())
                {
                    p.land(&anim);
                }
                self.loosed.clear();
                self.playing = self.next_anim();
            }
            self.stale = [true; 3];
            self.refresh();
            self.redraw();
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(40)));
            return;
        }
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
        bail!("usage: gboh-viewer <DATA dir> [MAP or SCN file] [ai|demo]  (no file: choose a battle)");
    };
    let base_palette = {
        let data = find_ci(&data_dir, "GAME.PAL").and_then(|p| std::fs::read(p).ok()).context("GAME.PAL not found")?;
        Palette::parse(&data)?
    };
    // no file: the setup screens choose the battle; `ai` after the file:
    // the computer plays the visitor army with its first plan; `demo`: it
    // plays both, paced by the viewer
    let (setup, battle) = match args.get(2) {
        None => (Some(setup::Setup::new(&data_dir)), None),
        Some(file) => {
            let computer = match args.get(3).map(String::as_str) {
                Some("ai") => [false, true],
                Some("demo") => [true, true],
                _ => [false, false],
            };
            (None, Some(load_battle(&data_dir, file, computer)?))
        }
    };
    let (title, map_file, scenes, canvases, play) = match battle {
        Some(b) => (b.title, b.map_file, b.scenes, b.canvases, b.play),
        None => (String::from("The Great Battles of Alexander"), String::new(), Vec::new(), vec![setup.as_ref().unwrap().canvas()], None),
    };
    let level = if setup.is_some() { 0 } else { 1 };
    let centre = [canvases[level].width as f32 / 2.0, canvases[level].height as f32 / 2.0];
    let mut viewer = Viewer {
        data_dir: data_dir.clone(),
        hud: hud::Hud::new(&data_dir),
        show_overview: true,
        details: None,
        results: None,
        game_overview: false,
        army: None,
        ending: false,
        ended: false,
        plain: None,
        quiet_missiles: false,
        quiet_finish: false,
        setup,
        quit: false,
        title,
        map_file,
        canvases,
        scenes,
        play,
        sound: sound::Sound::new(&data_dir),
        stale: [false; 3],
        playing: None,
        queued: Default::default(),
        runners: Vec::new(),
        loosed: Default::default(),
        next_step: Instant::now(),
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

/// A battle ready to show: its title, scenes and canvases for the three
/// zoom levels, and the game (for a scenario).
/// What a dialog popped up during play is for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    /// "Would you like to attempt momentum?"
    Momentum,
    /// Telling the player something.
    Notice(play::Notice),
}

struct Battle {
    title: String,
    map_file: String,
    scenes: Vec<Scene>,
    canvases: Vec<Canvas>,
    play: Option<play::Play>,
}

/// Load `file` (a scenario, or a bare map) from `data_dir`, the computer
/// playing the sides marked (home, visitor) with their first plans.
fn load_battle(data_dir: &Path, file: &str, computer: [bool; 2]) -> Result<Battle> {
    let load = |name: &str| find_ci(data_dir, name).and_then(|p| std::fs::read(p).ok());
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
    let views: Vec<UnitView> = units.iter().enumerate().filter_map(|(i, u)| UnitView::of_scenario(&map, i, u)).collect();
    let scenes = build_scenes(data_dir, map_file, &map, false)?;
    let canvases: Vec<Canvas> = scenes.iter().map(|s| s.compose(&views, &flagged)).collect();
    let mut ai = Vec::new();
    if let Some(s) = &scenario {
        let mut read = |name: &str| load(name).map(|d| String::from_utf8_lossy(&d).into_owned());
        for (nation, plans, on) in [(s.visitor.nation, &s.visitor_ai, computer[1]), (s.home.nation, &s.home_ai, computer[0])] {
            if !on {
                continue;
            }
            let plan = plans.first().context("no AI plan")?;
            let script = gboh_game::ai::Script::load(plan, &mut read).map_err(|e| anyhow::anyhow!("{e}"))?;
            ai.push((nation, gboh_game::ai::Machine::new(script)));
        }
    }
    let play = scenario.as_ref().map(|s| play::Play::new(gboh_game::game::Game::new(s, map.clone(), 1), ai));
    let title = if map.name.is_empty() { map_file.to_string() } else { map.name.clone() };
    Ok(Battle { title, map_file: map_file.to_string(), scenes, canvases, play })
}

/// The battlefield of `map` (loaded from `map_file`) at the three zooms,
/// reversed if `reversed`.
fn build_scenes(data_dir: &Path, map_file: &str, map: &Map, reversed: bool) -> Result<Vec<Scene>> {
    let mut load = |name: &str| find_ci(data_dir, name).and_then(|p| std::fs::read(p).ok());
    let titles = eif_title_index(data_dir);
    let mut load_title = |title: &str| titles.get(title).and_then(|n| std::fs::read(data_dir.join(n)).ok());
    let palette = Palette::parse(&load("GAME.PAL").context("GAME.PAL not found")?)?;
    let types: Vec<usize> = (0..ART_TITLES.len()).collect();
    Ok(ZOOMS
        .iter()
        .map(|&z| Scene::build_view(map_file, map, z, reversed, &palette, 1, &types, &mut load, &mut load_title))
        .collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every battle the setup screens offer loads; runs only with
    /// `GBOH_DATA` set.
    #[test]
    fn the_setup_battles_load() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        for file in setup::SCENARIOS {
            let b = load_battle(Path::new(&dir), file, [false, true]).unwrap_or_else(|e| panic!("{file}: {e:#}"));
            assert!(b.play.is_some(), "{file}");
            assert_eq!(b.canvases.len(), 3);
        }
    }
}
