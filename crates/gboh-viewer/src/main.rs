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

mod campaign;
mod editor;
mod hud;
mod library;
mod menu;
mod net;
mod play;
mod prefs;
mod saves;
mod setup;
mod shell;
mod slides;
mod sound;
mod ui;
mod video;
mod viewer;

use anyhow::{Context, Result, bail};
use gboh_formats::map::Map;
use gboh_formats::pal::Palette;
use gboh_formats::scn::Scenario;
use gboh_render::Canvas;
use gboh_render::battlefield::{self, Scene};
use gboh_render::terrain::Zoom;
use gboh_render::units::UnitView;
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
    tile_origin: vec2<f32>,
};

@group(0) @binding(0) var indices: texture_2d<u32>;
@group(0) @binding(1) var palette: texture_2d<f32>;
@group(0) @binding(2) var<uniform> view: View;
@group(0) @binding(3) var tile: texture_2d<u32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var p = floor(view.origin + pos.xy / view.scale);
    let outside = p.x < 0.0 || p.y < 0.0 || p.x >= view.size.x || p.y >= view.size.y;
    if (outside) {
        if (view.overlay > 0.5) {
            discard;
        }
        // past the battlefield's edge (when it is smaller than the window)
        // its gray background carries on, tiled as on the battlefield:
        // never a black margin
        let t = vec2<f32>(textureDimensions(tile));
        let q = p - view.tile_origin;
        let i = textureLoad(tile, vec2<i32>(q - t * floor(q / t)), 0).r;
        return textureLoad(palette, vec2<i32>(i32(i), 0), 0);
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
fn view_uniform(origin: [f32; 2], size: [f32; 2], scale: f32, overlay: bool, tile_origin: [f32; 2]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, v) in [origin[0], origin[1], size[0], size[1], scale, overlay as u8 as f32, tile_origin[0], tile_origin[1]].into_iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

/// One zoom level's battlefield on the GPU.
/// An index texture holding `pixels`.
fn index_texture(device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, pixels: &[u8]) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tile"),
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
        pixels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width), rows_per_image: None },
        texture.size(),
    );
    texture
}

struct Level {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    /// Where the background tile's tiling starts.
    tile_origin: [f32; 2],
}

/// A level's background tile (shown past its edges) and where its tiling
/// starts.
type Tile = (gboh_render::Sprite, (i32, i32));

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
    /// A one-pixel tile for levels with none (and the overlay).
    no_tile: wgpu::TextureView,
}

impl Gpu {
    fn new(window: Arc<Window>, canvases: &[Canvas], tiles: &[Option<Tile>]) -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { compatible_surface: Some(&surface), ..Default::default() }))
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
        let mut config = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).context("surface not supported")?;
        // palette colours are written as-is, so avoid an sRGB conversion
        let caps = surface.get_capabilities(&adapter);
        if let Some(&f) = caps.formats.iter().find(|f| !f.is_srgb()) {
            config.format = f;
        }
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let texture_entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
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
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                texture_entry(3, wgpu::TextureSampleType::Uint),
            ],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
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
        let no_tile = index_texture(&device, &queue, 1, 1, &[0]).create_view(&Default::default());
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
            no_tile,
        };
        gpu.set_levels(canvases, tiles);
        Ok(gpu)
    }

    /// New textures for `canvases` (a battle's zoom levels, or a screen),
    /// each with the tile shown past its edges (none: black).
    fn set_levels(&mut self, canvases: &[Canvas], tiles: &[Option<Tile>]) {
        self.levels = canvases
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let (width, height) = (c.width as u32, c.height as u32);
                let (tile, tile_origin) = match tiles.get(k).and_then(Option::as_ref) {
                    Some((s, (x, y))) => (index_texture(&self.device, &self.queue, s.width as u32, s.height as u32, &s.pixels), [*x as f32, *y as f32]),
                    None => (index_texture(&self.device, &self.queue, 1, 1, &[0]), [0.0, 0.0]),
                };
                let tile_view = tile.create_view(&Default::default());
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
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&tile_view) },
                    ],
                });
                Level { width, height, texture, bind_group, tile_origin }
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
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.no_tile) },
                ],
            });
            Level { width, height, texture, bind_group, tile_origin: [0.0, 0.0] }
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
        self.queue.write_buffer(&self.uniform, 0, &view_uniform(origin, [l.width as f32, l.height as f32], scale, false, l.tile_origin));
        if let Some(o) = &self.overlay {
            self.queue.write_buffer(&self.overlay_uniform, 0, &view_uniform([0.0, 0.0], [o.width as f32, o.height as f32], scale, true, [0.0, 0.0]));
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
    /// The battle being played (its scenario file) and who the computer
    /// plays, for saving; and the last turn autosaved.
    battle: (String, [bool; 2]),
    autosaved: u16,
    /// The player's preferences.
    prefs: prefs::Prefs,
    /// The menu bar and the menu open on it.
    menu_bar: menu::MenuBar,
    menu_open: Option<menu::Open>,
    /// Whether the running commentary shows.
    show_commentary: bool,
    /// The file last saved to, for Save.
    save_name: Option<(String, String)>,
    /// A slider of the dialog up being dragged.
    sliding: Option<usize>,
    /// A text window open (help, credits, a battle's history), and the
    /// help file once read.
    reader: Option<hud::Reader>,
    help: Option<gboh_formats::hlp::Help>,
    /// The game's cursors by name, the one shown, and the way the view
    /// scrolls with the mouse at the window's edge.
    /// The campaign under way, its art, and its map or results screen
    /// while up.
    campaign: Option<gboh_game::campaign::Campaign>,
    campaign_art: Option<campaign::CampaignArt>,
    campaign_map: Option<campaign::MapState>,
    /// The campaign map's last press: when and where (to tell a
    /// double-click).
    map_press: Option<(std::time::Instant, (i32, i32))>,
    campaign_results: Option<gboh_game::campaign::Results>,
    /// A network game: setting up or under way.
    net: Option<NetGame>,
    /// The campaign's victory video, while it plays.
    video: Option<video::Video>,
    /// The intro or the historical video, while it plays.
    slides: Option<slides::Slides>,
    /// Whether the video playing is the intro (over the whole window).
    intro: bool,
    cursors: std::collections::HashMap<&'static str, winit::window::CustomCursor>,
    cursor_shown: Option<&'static str>,
    edge: Option<(f32, f32)>,
    /// When the flags' pennants next wave.
    next_flag: Instant,
    /// The rewrite's own screen up (content manager, settings, game
    /// selector), its buttons, a line of news, an import under way, the
    /// settings, and the title whose data is in use.
    shell: Option<shell::Screen>,
    /// The scenario editor, while open.
    editor: Option<editor::EditorScreen>,
    /// The next battle begins with Hannibal's deployment phase.
    deploy_next: bool,
    /// The rule options of the next battle started, if not the
    /// settings' (a saved game's).
    options_next: Option<gboh_game::options::Options>,
    /// The options a network game's host sent for the next game.
    net_options: Option<gboh_game::options::Options>,
    /// Those for the network game being started.
    net_start: Option<gboh_game::options::Options>,
    shell_buttons: Vec<shell::Button>,
    shell_pressed: Option<(i32, i32)>,
    /// For testing: clicks (or drags) to make, in the window's physical
    /// pixels, two seconds apart (`GBOH_CLICKS="x,y;x,y>x2,y2"`).
    script: std::collections::VecDeque<ScriptStep>,
    script_next: Instant,
    news: (String, bool),
    importing: Option<(library::Title, std::sync::Arc<library::Progress>)>,
    settings: library::Settings,
    game: Option<library::Title>,
    /// The display's scale (battlefield pixels per window pixel), and
    /// where the setup screen lies on its fitted canvas.
    display_scale: f32,
    setup_offset: (i32, i32),
    /// The toolbar button the mouse rests on, since when, and whether its
    /// tooltip shows.
    hover: Option<(hud::Tool, Instant, bool)>,
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
    /// The keyboard's modifier keys held.
    modifiers: winit::keyboard::ModifiersState,
}

impl Viewer {
    fn origin(&self) -> [f32; 2] {
        let size = self.gpu.as_ref().map_or(PhysicalSize::new(1, 1), |g| g.window.inner_size());
        let c = self.clamped(self.centre);
        [c[0] - size.width as f32 / 2.0 / self.scale, c[1] - size.height as f32 / 2.0 / self.scale]
    }

    /// `centre` kept so that the window shows only the canvas: no further
    /// than half a window from its edges (or its middle, if the canvas is
    /// the smaller).
    fn clamped(&self, centre: [f32; 2]) -> [f32; 2] {
        let size = self.gpu.as_ref().map_or(PhysicalSize::new(1, 1), |g| g.window.inner_size());
        let Some(c) = self.canvases.get(self.level) else { return centre };
        let axis = |v: f32, canvas: usize, window: u32| {
            let (canvas, half) = (canvas as f32, window as f32 / 2.0 / self.scale);
            if canvas <= 2.0 * half { canvas / 2.0 } else { v.clamp(half, canvas - half) }
        };
        [axis(centre[0], c.width, size.width), axis(centre[1], c.height, size.height)]
    }

    fn scroll(&mut self, dx: f32, dy: f32) {
        // the setup screens fit the window and do not scroll
        if self.setup.is_some() {
            return;
        }
        self.centre = self.clamped([self.centre[0] + dx / self.scale, self.centre[1] + dy / self.scale]);
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
        if let (Some(g), Some(_)) = (&self.gpu, &self.shell) {
            g.window.set_title("opengboh");
            return;
        }
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
            // what the preferences leave unanimated lands at once
            for a in std::mem::take(&mut p.anims) {
                let shown = match a {
                    play::Anim::Walk { .. } => self.prefs.movement_animation,
                    _ => self.prefs.combat_animation,
                };
                if shown {
                    self.queued.push_back(a);
                } else {
                    p.land(&a);
                }
            }
            if self.playing.is_none() {
                self.playing = self.next_anim();
            }
        }
        self.autosave();
        self.send_orders();
        self.stale = [true; 3];
        self.refresh();
        self.next_notice();
        self.update_title();
        self.update_hud();
        self.redraw();
    }

    /// A click at the cursor.
    fn click(&mut self, right: bool) {
        if self.editor.is_some() {
            if !right {
                self.editor_click();
            }
            return;
        }
        if self.shell.is_some() {
            let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
            if let Some(id) = shell::button_at(&self.shell_buttons, cx, cy) {
                self.shell_click(id);
            }
            return;
        }
        let o = self.origin();
        let (px, py) = (o[0] + self.cursor.x as f32 / self.scale, o[1] + self.cursor.y as f32 / self.scale);
        // a text window over the start screen: Close, Contents, links
        if self.setup.is_some()
            && self.plain.is_none()
            && let Some(r) = self.reader.clone()
        {
            let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
            let (w, h) = self.window_px();
            match self.hud.reader_at(&r, w, h, cx, cy) {
                Some(hud::ReaderClick::Button(0)) => self.reader = None,
                Some(hud::ReaderClick::Button(_)) => self.open_help(),
                Some(hud::ReaderClick::Link(t)) => self.open_topic(t),
                None => {}
            }
            self.update_hud();
            self.redraw();
            return;
        }
        // the campaign's results: a click closes them
        if self.setup.is_some() && self.plain.is_none() && self.campaign_results.is_some() {
            self.campaign_results = None;
            self.sound.set_music(sound::Music::Start);
            self.update_hud();
            self.redraw();
            return;
        }
        // the campaign map's buttons (its dragging is in the mouse events)
        if self.setup.is_some() && self.plain.is_none() && self.campaign_map.is_some() {
            let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
            let (w, h) = self.window_px();
            let at = self.campaign_art.as_ref().and_then(|a| a.button_at(w, h, cx, cy));
            self.campaign_button(at);
            return;
        }
        // a dialog over the start screen takes clicks first
        if self.setup.is_some()
            && let Some((d, asked)) = self.plain.clone()
        {
            let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
            let (w, h) = self.window_px();
            let at = self.hud.plain_at(&d, w, h, cx, cy);
            match asked {
                Asked::Files { save, entries } => self.files_click(save, &entries, &d, at, (w, h, cx, cy)),
                Asked::Preferences => self.preferences_click(&d, at, (w, h, cx, cy)),
                Asked::Message => {
                    if matches!(at, Some(1 | 2)) {
                        self.plain = None;
                    }
                }
                Asked::Custom { files } => self.custom_click(&files, &d, at, (w, h, cx, cy)),
                Asked::Variants { files } => self.variant_click(&files, &d, at, (w, h, cx, cy)),
                Asked::Network(_) => self.network_click(&d, at, (w, h, cx, cy)),
                Asked::Video if at == Some(1) => self.close_video(),
                _ => {}
            }
            self.update_hud();
            self.redraw();
            return;
        }
        if let Some(setup) = &mut self.setup {
            match setup.click(px as i32 - self.setup_offset.0, py as i32 - self.setup_offset.1) {
                setup::Outcome::Nothing => {}
                setup::Outcome::Redraw => self.show_setup(),
                setup::Outcome::Quit => self.quit = true,
                setup::Outcome::Load => self.open_files(false),
                setup::Outcome::Variants(v) => self.choose_variant(v),
                setup::Outcome::Play { .. } | setup::Outcome::Campaign | setup::Outcome::Multiplayer if !self.game.is_none_or(|g| g.playable()) => {
                    self.notice(&["The rewrite does not play this game's battles yet."]);
                }
                setup::Outcome::Play { scenario, computer, levels } => {
                    if let Some(n) = &mut self.net {
                        // a network game: both armies played by hand, once
                        // the other player is there
                        // the sides marked Computer go to the other player
                        n.chosen = Some((scenario, levels, computer));
                        if n.link.is_some() {
                            self.host_start();
                        } else {
                            self.message(&[109]);
                        }
                    } else {
                        // Hannibal's set-up: the player deploys his army
                        self.deploy_next = self.setup.as_ref().is_some_and(|s| !s.default_deployment);
                        if let Err(e) = self.start_battle(&scenario, computer, levels) {
                            eprintln!("error: {e:#}");
                        }
                    }
                }
                setup::Outcome::Message(ids) if ids == [83] => self.manual_note(),
                setup::Outcome::Message(ids) => self.message(&ids),
                setup::Outcome::Tutorial => self.tutorial(),
                setup::Outcome::History => self.run_program("HISTORY.EXE"),
                setup::Outcome::Custom => self.choose_custom(),
                setup::Outcome::Campaign => self.start_campaign(),
                setup::Outcome::Multiplayer => self.open_network(),
                setup::Outcome::Content => self.open_shell(shell::Screen::Content),
                setup::Outcome::Settings => self.open_shell(shell::Screen::Settings),
                setup::Outcome::Selector => {
                    let entries = shell::entries(&library::imported());
                    self.open_shell(shell::Screen::Selector { entries, scroll: 0 });
                }
                setup::Outcome::Editor => self.open_editor(),
            }
            return;
        }
        // the toolbar and status bar take clicks over them
        let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
        let (w, h) = self.window_px();
        // the menu bar: an open menu takes the click (a command, another
        // menu, or closing it); else the bar opens one, unless a dialog is up
        if let Some((k, _)) = self.menu_open {
            self.menu_open = None;
            if let Some(i) = self.menu_bar.item_at(k, cx, cy) {
                if let Some(id) = self.menu_bar.id(k, i).filter(|&id| self.command_enabled(id)) {
                    self.command(id);
                }
            } else if let Some(j) = self.menu_bar.bar_at(cx, cy).filter(|&j| j != k) {
                self.menu_open = Some((j, None));
            }
            self.update_hud();
            self.redraw();
            return;
        }
        if cy < menu::MENU_HEIGHT {
            if !self.modal()
                && let Some(k) = self.menu_bar.bar_at(cx, cy)
            {
                self.menu_open = Some((k, None));
                self.update_hud();
                self.redraw();
            }
            return;
        }
        // a dialog popped up during play is modal: its buttons and check
        // boxes; the finish reminder also closes on a click off it
        if let Some((d, asked)) = self.plain.clone() {
            let at = self.hud.plain_at(&d, w, h, cx, cy);
            if let Asked::Files { save, entries } = &asked {
                self.files_click(*save, entries, &d, at, (w, h, cx, cy));
                return;
            }
            match asked {
                Asked::Preferences => return self.preferences_click(&d, at, (w, h, cx, cy)),
                Asked::Message => {
                    if matches!(at, Some(1 | 2)) {
                        self.plain = None;
                        self.update_hud();
                        self.redraw();
                    }
                    return;
                }
                Asked::Chat => {
                    match at {
                        Some(108) => self.plain_ok(&d, &Asked::Chat),
                        Some(107) => {}
                        Some(9) => self.open_help(),
                        Some(_) => {}
                        None => self.plain = None,
                    }
                    self.update_hud();
                    self.redraw();
                    return;
                }
                _ => {}
            }
            match (asked, at) {
                (Asked::Momentum, Some(1 | 2)) => {
                    self.plain = None;
                    if self.play.as_mut().is_some_and(|p| p.finish(at == Some(1))) {
                        self.changed();
                    }
                }
                (Asked::Notice(play::Notice::NoOrders), Some(101)) => {
                    self.set_quiet_finish(d.checked.contains(&17));
                    self.plain = None;
                    self.finish_leader();
                }
                (Asked::Group(order), Some(a @ (1 | 2))) => {
                    if d.checked.contains(&17) && self.prefs.confirm_group {
                        self.prefs.confirm_group = false;
                        self.save_prefs();
                    }
                    self.plain = None;
                    if a == 1 && self.play.as_mut().is_some_and(|p| p.group_order(order)) {
                        self.changed();
                    }
                }
                (Asked::Group(_), None) => return,
                (Asked::Notice(play::Notice::Elite { .. }), Some(a @ (1 | 2))) => {
                    self.plain = None;
                    if let Some(p) = &mut self.play {
                        p.elite(a == 1);
                    }
                    self.changed();
                }
                (Asked::Notice(play::Notice::Elite { .. }), None) => return,
                (Asked::Notice(play::Notice::NoOrders), None) => {
                    self.set_quiet_finish(d.checked.contains(&17));
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
                    if d.checked.contains(&201) && !self.prefs.quiet_missiles {
                        self.prefs.quiet_missiles = true;
                        self.save_prefs();
                    }
                    self.plain = None;
                }
                _ => return,
            }
            self.next_notice();
            self.update_hud();
            self.redraw();
            return;
        }
        // the text window: its buttons (Close, the help's Contents) and the
        // help's links
        if let Some(r) = self.reader.clone() {
            match self.hud.reader_at(&r, w, h, cx, cy) {
                Some(hud::ReaderClick::Button(0)) => self.reader = None,
                Some(hud::ReaderClick::Button(_)) => self.open_help(),
                Some(hud::ReaderClick::Link(t)) => self.open_topic(t),
                None => return,
            }
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
                    let player = self.campaign.as_ref().map_or(MACEDONIAN, |c| c.player_nation());
                    let won = self.play.as_ref().and_then(|p| p.winner.map(|a| p.game.armies[a].0 == player));
                    self.back_to_start();
                    if let (Some(c), Some(won)) = (&mut self.campaign, won) {
                        let next = c.after_battle(won);
                        self.campaign_next(next);
                    }
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
        if self.play.is_some() && cy < hud::TOP {
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
        if !self.my_turn() {
            return;
        }
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
        if self.shell.is_some() || self.editor.is_some() {
            if let Some(g) = &mut self.gpu {
                g.set_overlay(None);
            }
            return;
        }
        // the intro: its frame in the middle of the window, as large as
        // whole pixels allow, on black
        if let (true, Some(v)) = (self.intro, &self.video) {
            let mut c = Canvas::new(w, h, v.black());
            if let Some(f) = &v.shown {
                let k = (w / f.width).min(h / f.height).max(1);
                let big = gboh_render::Sprite {
                    width: f.width * k,
                    height: f.height * k,
                    pixels: (0..f.height * k).flat_map(|y| (0..f.width * k).map(move |x| (x, y))).map(|(x, y)| f.pixels[(y / k) * f.width + x / k]).collect(),
                };
                c.blit(&big, (w as i32 - big.width as i32) / 2, (h as i32 - big.height as i32) / 2);
            }
            if let Some(g) = &mut self.gpu {
                g.set_overlay(Some(&c));
            }
            return;
        }
        if let Some(s) = &self.slides {
            let c = s.canvas(w, h);
            if let Some(g) = &mut self.gpu {
                g.set_overlay(Some(&c));
            }
            return;
        }
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
                        || (t == AboutFace && p.can_about_face())
                        || (t == Undo && p.can_undo())
                };
                let turn = format!("Turn {}", p.game.activation.turn + 1);
                // the button under the mouse describes itself in the status bar
                let grid = self.scenes.get(self.level).is_some_and(|s| s.grid_on);
                let hover_line = self.hover.and_then(|(t, _, _)| self.hud.tip(t, self.show_overview, grid)).map(|(_, l)| l);
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
                    [&hover_line.unwrap_or_else(|| p.status.clone()), &turn, zoom],
                    overview.as_ref(),
                    self.details.as_ref(),
                    results.as_ref(),
                    game_overview.as_ref(),
                    if self.show_commentary { &p.commentary } else { &[] },
                    army.as_ref(),
                    self.ending.then(|| p.ending()).flatten(),
                    self.plain.as_ref().map(|(d, _)| d),
                    self.reader.as_ref(),
                ))
                .map(|mut c| {
                    self.menu_bar.draw(&mut c, self.menu_open, |id| self.command_enabled(id), |id| self.command_checked(id));
                    c
                })
            }
            (Some(_), _) => match (&self.plain, &self.reader) {
                (Some((d, _)), _) => {
                    let mut c = self.campaign_overlay(w, h).unwrap_or_else(|| Canvas::new(w, h, 0));
                    let dialog = self.hud.dialog_canvas(w, h, d);
                    c.blit(&gboh_render::Sprite { width: dialog.width, height: dialog.height, pixels: dialog.pixels }, 0, 0);
                    // the video's frame in its window, 32 × 10 pixels in
                    if let (Some(v), Some((x, y))) = (self.video.as_ref().and_then(|v| v.shown.as_ref()), self.hud.plain_origin(d, w, h)) {
                        c.blit(v, x + 32, y + 10);
                    }
                    Some(c)
                }
                (None, Some(r)) => {
                    let mut c = self.campaign_overlay(w, h).unwrap_or_else(|| Canvas::new(w, h, 0));
                    let reader = self.hud.reader_canvas(w, h, r);
                    c.blit(&gboh_render::Sprite { width: reader.width, height: reader.height, pixels: reader.pixels }, 0, 0);
                    Some(c)
                }
                _ => self.campaign_overlay(w, h),
            },
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
        let every = p.game.battle.layout == gboh_formats::scn::Layout::Editor;
        let scenes = match build_scenes(&self.data_dir, &self.map_file, &p.game.map, reversed, every) {
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
            s.sparse = self.prefs.sparse_trees;
        }
        // the background's tiling starts elsewhere on the turned field
        let tiles = self.tiles();
        if let Some(g) = &mut self.gpu {
            g.set_levels(&self.canvases, &tiles);
        }
        self.stale = [true; 3];
        self.refresh();
    }

    /// A toolbar button pressed.
    fn tool(&mut self, tool: hud::Tool) {
        use hud::Tool::*;
        let orders = matches!(tool, FinishLeader | RotateLeft | RotateRight | AboutFace | MissileVolley | RestoreCohesion | Rally | Undo | BackwardMovement);
        if orders && !self.my_turn() {
            return;
        }
        let Some(p) = &mut self.play else { return };
        let changed = match tool {
            FinishLeader => {
                self.finish_leader();
                false
            }
            AboutFace => p.about_face(),
            Undo => p.undo(),
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
                                // (🟡 Caesar's axes named as javelins)
                                Javelin | MountedJavelin | Axe => 66,
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
            CloseGame => {
                self.command(211);
                return;
            }
            GroupMove => {
                self.command(212);
                return;
            }
            LineExtension => {
                self.command(247);
                return;
            }
            GroupAttack => {
                self.command(259);
                return;
            }
            BackwardMovement => p.arm_backward(),
            _ => false,
        };
        if changed {
            self.changed();
        }
        self.update_hud();
        self.redraw();
    }

    /// Whether a dialog is up that keeps the rest of the window from
    /// clicks (and the menus from opening).
    fn modal(&self) -> bool {
        self.plain.is_some()
            || self.reader.is_some()
            || self.army.is_some()
            || self.game_overview
            || self.ending
            || self.results.is_some()
            || self.details.is_some()
    }

    /// Whether menu command `id` can be chosen now.
    fn command_enabled(&self, id: u16) -> bool {
        let Some(p) = &self.play else { return matches!(id, 101 | 102 | 238) };
        let selected = p.selected().is_some();
        let tool = |t: hud::Tool| {
            use hud::Tool::*;
            match t {
                UnitDetails => selected,
                MissileVolley => p.can_fire(),
                AboutFace => p.can_about_face(),
                Undo => p.can_undo(),
                GroupMove => p.game.group_move_refused().is_none(),
                LineExtension => p.game.can_extend_line(),
                GroupAttack => p.game.group_orders_allowed() && p.game.points.as_ref().is_some_and(|c| c.used == 0),
                BackwardMovement => p.can_back(),
                _ => true,
            }
        };
        match id {
            // a second map window, the windows' arrangement and the network
            // chat have no place in the one window of the rewrite
            254 | 104..=107 => false,
            241 => self.net.as_ref().is_some_and(|n| n.link.is_some()),
            226 | 227 => !p.over,
            209 | 224 => !p.over && p.leader().is_some(),
            212 => p.game.group_move_refused().is_none(),
            247 => p.game.can_extend_line(),
            248 | 259 | 260 => p.game.group_orders_allowed() && p.game.points.as_ref().is_some_and(|c| c.used == 0),
            246 => p.can_back(),
            222 | 213 | 214 | 220 | 221 => selected,
            225 => tool(hud::Tool::UnitDetails),
            219 => tool(hud::Tool::MissileVolley),
            235 => tool(hud::Tool::AboutFace),
            255 => tool(hud::Tool::Undo),
            250 => self.show_overview,
            251 => !self.show_overview,
            236 | 237 => true,
            _ => true,
        }
    }

    /// Whether menu command `id` shows ticked.
    fn command_checked(&self, id: u16) -> bool {
        match id {
            239 => self.show_commentary,
            223 => self.scenes.get(self.level).is_some_and(|s| s.layout.reversed),
            216..=218 => self.level == (218 - id) as usize,
            _ => false,
        }
    }

    /// A menu command (ids of the original's `CHILDMENU`).
    fn command(&mut self, id: u16) {
        use hud::Tool;
        match id {
            101 => self.quit = true,
            // closing the battle of a campaign or a network game ends it
            211 => {
                self.campaign = None;
                self.net = None;
                self.back_to_start();
            }
            241 => self.open_chat(),
            226 => match self.save_name.clone() {
                Some((file, description)) if self.play.is_some() => self.files_ok(true, &file, &description),
                _ => self.open_files(true),
            },
            227 => self.open_files(true),
            229 => self.game_overview = self.play.is_some(),
            230 => self.open_preferences(),
            209 => self.tool(Tool::CenterOnLeader),
            212 => {
                if self.my_turn() && self.play.as_mut().is_some_and(|p| p.group_move()) {
                    self.changed();
                }
            }
            // Hannibal's: line extension, group retreat, attack, restore
            // cohesion
            246 if self.play.as_ref().is_some_and(|p| p.game.rules.newer() || p.game.battle.options.backward) => {
                if self.my_turn()
                    && let Some(p) = &mut self.play
                {
                    p.arm_backward();
                }
            }
            247 | 248 | 259 | 260 if self.play.as_ref().is_some_and(|p| p.game.group_orders_allowed()) => {
                use play::GroupOrder::*;
                let order = match id {
                    247 => LineExtension,
                    248 => Retreat,
                    259 => Attack,
                    _ => RestoreCohesion,
                };
                if !self.my_turn() {
                } else if self.prefs.confirm_group {
                    // "this uses all of the leader's orders": OK or Cancel
                    self.plain = Some((hud::Plain { name: "GROUPCOMMANDCONFIRM", ..Default::default() }, Asked::Group(order)));
                    self.redraw();
                } else if self.play.as_mut().is_some_and(|p| p.group_order(order)) {
                    self.changed();
                }
            }
            224 => self.finish_leader(),
            222 => {
                if let Some(u) = self.play.as_ref().and_then(|p| p.selected()) {
                    let h = self.play.as_ref().unwrap().game.battle.units[u].hex;
                    let scene = &self.scenes[self.level];
                    let (x, y) = gboh_render::objects::anchor_at(&scene.map, &scene.layout, (h.x.max(0) as usize, h.y.max(0) as usize), (0, 0));
                    self.centre = [x as f32, y as f32];
                }
            }
            225 => self.tool(Tool::UnitDetails),
            255 => self.tool(Tool::Undo),
            213 => self.tool(Tool::Rally),
            214 => self.tool(Tool::RestoreCohesion),
            219 => self.tool(Tool::MissileVolley),
            220 => self.tool(Tool::RotateLeft),
            221 => self.tool(Tool::RotateRight),
            235 => self.tool(Tool::AboutFace),
            // the bodies lying about are cleared away (the original's
            // RemoveDeadSprites)
            253 => {
                if let Some(p) = &mut self.play {
                    p.corpses.clear();
                }
                self.stale = [true; 3];
                self.refresh();
            }
            218 => self.tool(Tool::ZoomIn),
            217 => self.tool(Tool::ZoomNormal),
            216 => self.tool(Tool::ZoomOut),
            223 => self.tool(Tool::ReverseView),
            250 | 251 => self.show_overview = id == 251,
            239 => self.show_commentary = !self.show_commentary,
            102 => self.show_info(Info::About),
            236 => self.show_info(Info::Battle),
            237 => self.show_info(Info::Victory),
            238 => self.open_help(),
            _ => {}
        }
        self.update_hud();
        self.redraw();
    }

    /// The help file, read on first use.
    fn help(&mut self) -> Option<&gboh_formats::hlp::Help> {
        if self.help.is_none() {
            let game = gboh_formats::game::Game::detect(&self.data_dir).unwrap_or_default();
            let path = find_ci(&self.data_dir, game.help_file())?;
            self.help = std::fs::read(path).ok().and_then(|d| gboh_formats::hlp::Help::parse(&d).ok());
        }
        self.help.as_ref()
    }

    /// The caption of the about box's Close button (from `alex.exe`).
    fn close_caption(&self) -> String {
        crate::ui::exe_resources(&self.data_dir)
            .and_then(|r| r.dialog("ABOUTDLG"))
            .and_then(|d| d.controls.iter().find(|c| c.id == 1).map(|c| c.text.clone()))
            .unwrap_or_else(|| "&Close".into())
    }

    /// The help's contents: its topics with titles, each a link (WinHelp's
    /// own contents page is drawn by an extension DLL the rewrite does not
    /// have, so the list is made from the topics).
    fn open_help(&mut self) {
        let close = self.close_caption();
        let Some(help) = self.help() else { return };
        let topics: Vec<(usize, String)> =
            help.topics.iter().enumerate().filter(|(_, t)| !t.title.trim().is_empty()).map(|(k, t)| (k, t.title.clone())).collect();
        let title = help.title.clone();
        self.reader = Some(hud::Reader {
            title,
            paragraphs: topics.iter().map(|(_, t)| t.clone()).collect(),
            links: topics.iter().map(|(k, _)| Some(*k)).collect(),
            scroll: 0,
            buttons: vec![close],
        });
    }

    /// Help topic `k`, with a button back to the contents.
    fn open_topic(&mut self, k: usize) {
        let close = self.close_caption();
        let Some(t) = self.help().and_then(|h| h.topics.get(k)).cloned() else { return };
        // a popup's title is its first line
        let title = if t.title.trim().is_empty() { t.paragraphs.first().cloned().unwrap_or_default() } else { t.title.clone() };
        self.reader =
            Some(hud::Reader { title, links: vec![None; t.paragraphs.len()], paragraphs: t.paragraphs, scroll: 0, buttons: vec![close, "Contents".into()] });
    }

    /// The about box (`ABOUTDLG`: its texts and the credits, `CREDITS.RTF`),
    /// or the battle's history or victory conditions from the help's
    /// battle topics ("<Battle> - <year> BC" and "<Battle>  (<year> BC)");
    /// a battle the help does not cover (a custom one) gets the original's
    /// string 83.
    fn show_info(&mut self, info: Info) {
        let close = self.close_caption();
        let res = crate::ui::exe_resources(&self.data_dir);
        let (title, paragraphs) = match info {
            Info::About => {
                let d = res.as_ref().and_then(|r| r.dialog("ABOUTDLG"));
                let mut texts: Vec<(i16, String)> =
                    d.iter().flat_map(|d| d.controls.iter()).filter(|c| c.id == 0xffff).map(|c| (c.rect.1, c.text.clone())).collect();
                texts.sort();
                let credits =
                    find_ci(&self.data_dir, "CREDITS.RTF").and_then(|p| std::fs::read(p).ok()).map(|d| gboh_formats::rtf::paragraphs(&d)).unwrap_or_default();
                let mut paragraphs: Vec<String> = texts.into_iter().map(|(_, t)| t).collect();
                paragraphs.extend(credits);
                (d.map(|d| d.title).unwrap_or_default(), paragraphs)
            }
            Info::Battle | Info::Victory => {
                let name = self.title.to_ascii_lowercase();
                let found = self.help().and_then(|h| {
                    h.topics
                        .iter()
                        .find(|t| {
                            let key = match info {
                                Info::Battle => t.title.split(" - ").next().filter(|_| t.title.contains(" - ")),
                                _ => t.title.split(" (").next().filter(|_| t.title.contains(" (") && t.title.contains("BC)")),
                            };
                            key.is_some_and(|k| !k.trim().is_empty() && name.contains(&k.trim().to_ascii_lowercase()))
                        })
                        .cloned()
                });
                match found {
                    Some(t) => {
                        let paragraphs = match info {
                            Info::Victory => {
                                let from = t.paragraphs.iter().position(|p| p.to_ascii_uppercase().contains("VICTORY CONDITIONS")).unwrap_or(0);
                                t.paragraphs[from..].to_vec()
                            }
                            _ => t.paragraphs.clone(),
                        };
                        (t.title.clone(), paragraphs)
                    }
                    None => (self.title.clone(), vec![res.as_ref().and_then(|r| r.string(83)).unwrap_or_default()]),
                }
            }
        };
        self.reader = Some(hud::Reader { title, links: vec![None; paragraphs.len()], paragraphs, scroll: 0, buttons: vec![close] });
    }

    /// Note where the left button went down (or that it came up), for the
    /// buttons drawn pressed, and redraw what shows them.
    fn set_pressed(&mut self, down: bool) {
        let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
        let at = down.then_some((cx, cy));
        self.hud.pressed.set(at);
        // a click puts the tooltip away until the mouse moves off
        if down {
            *self.hud.tooltip.borrow_mut() = None;
            if let Some(h) = &mut self.hover {
                h.2 = true;
            }
        }
        if let Some(m) = &mut self.campaign_map {
            m.pressed = at;
        }
        let o = self.origin();
        if self.shell.is_some() {
            self.shell_pressed = at;
            self.show_shell();
            return;
        }
        let off = self.setup_offset;
        if let Some(setup) = &mut self.setup {
            setup.pressed = down.then(|| ((o[0] + cx as f32) as i32 - off.0, (o[1] + cy as f32) as i32 - off.1));
            if let Some(canvas) = self.setup_canvas() {
                if let Some(g) = &self.gpu {
                    g.update_level(0, &canvas);
                }
                self.canvases[0] = canvas;
            }
        }
        self.update_hud();
        self.redraw();
    }

    /// Choose the cursor for where the mouse is: at the battlefield's edge
    /// an arrow the way it scrolls (the original's `Move…` cursors), over
    /// a target the sword or a missile, else the arrow.
    fn update_cursor(&mut self) {
        let (w, h) = self.window_px();
        let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
        let playing = self.play.is_some() && self.setup.is_none() && !self.modal() && self.menu_open.is_none() && self.drag.is_none();
        let field = playing && cy >= hud::TOP && cy < h as i32 - hud::STATUS_HEIGHT;
        // the toolbar button under the mouse, for its tooltip
        let tool = (self.play.is_some() && self.setup.is_none() && self.menu_open.is_none()).then(|| self.hud.tool_at(cx, cy)).flatten();
        match (tool, self.hover) {
            (Some(t), Some((h, _, _))) if t == h => {}
            (Some(t), _) => self.hover = Some((t, Instant::now(), false)),
            (None, Some((_, _, shown))) => {
                self.hover = None;
                if shown {
                    *self.hud.tooltip.borrow_mut() = None;
                    self.update_hud();
                    self.redraw();
                }
            }
            (None, None) => {}
        }
        // the original's margins: 25 pixels across, 35 down, inside the
        // map window (set where the map view starts, `DAT_005ec7c4`)
        const EDGE: (i32, i32) = (25, 35);
        let (left, right) = (cx < EDGE.0, cx >= w as i32 - EDGE.0);
        let (up, down) = (cy < hud::TOP + EDGE.1, cy >= h as i32 - hud::STATUS_HEIGHT - EDGE.1);
        let dx = if left {
            -1.0
        } else if right {
            1.0
        } else {
            0.0
        };
        let dy = if up {
            -1.0
        } else if down {
            1.0
        } else {
            0.0
        };
        self.edge = (field && (dx != 0.0 || dy != 0.0)).then_some((dx, dy));
        let name = match self.edge {
            Some((dx, dy)) => Some(match (dx as i32, dy as i32) {
                (0, -1) => "MOVENORTH",
                (1, -1) => "MOVENORTHEAST",
                (1, 0) => "MOVEEAST",
                (1, 1) => "MOVESOUTHEAST",
                (0, 1) => "MOVESOUTH",
                (-1, 1) => "MOVESOUTHWEST",
                (-1, 0) => "MOVEWEST",
                _ => "MOVENORTHWEST",
            }),
            None if field => {
                let o = self.origin();
                let (px, py) = (o[0] + cx as f32, o[1] + cy as f32);
                play::pick(&self.scenes[self.level], px as i32, py as i32).and_then(|hex| self.play.as_ref()?.cursor_at(hex))
            }
            None => None,
        };
        if name != self.cursor_shown {
            self.cursor_shown = name;
            if let Some(g) = &self.gpu {
                match name.and_then(|n| self.cursors.get(n)) {
                    Some(c) => g.window.set_cursor(c.clone()),
                    None => g.window.set_cursor(winit::window::CursorIcon::Default),
                }
            }
        }
    }

    /// Tell the player strings `ids` of the string table (the original's
    /// `MESSAGEBOX`: up to three lines, OK).
    fn message(&mut self, ids: &[u16]) {
        let res = crate::ui::exe_resources(&self.data_dir);
        let texts = ids.iter().take(3).enumerate().map(|(k, &id)| (101 + k as u16, res.as_ref().and_then(|r| r.string(id)).unwrap_or_default())).collect();
        self.plain = Some((hud::Plain { name: "MESSAGEBOX", texts, ..Default::default() }, Asked::Message));
        self.update_hud();
        self.redraw();
    }

    /// Point the player to the game's manual (what the GOG build's help
    /// and details do): Alexander's string 83; the other games have no such
    /// string (Hannibal's 83 is a nation's name), so in the rewrite's words.
    fn manual_note(&mut self) {
        if gboh_formats::game::Game::detect(&self.data_dir).unwrap_or_default() == gboh_formats::game::Game::Alexander {
            self.message(&[83]);
        } else {
            self.notice(&["See the game's manual for details."]);
        }
    }

    /// Tell the player something in the rewrite's own words (the
    /// original's `MESSAGEBOX`, up to three lines).
    fn notice(&mut self, lines: &[&str]) {
        let texts = lines.iter().take(3).enumerate().map(|(k, l)| (101 + k as u16, l.to_string())).collect();
        self.plain = Some((hud::Plain { name: "MESSAGEBOX", texts, ..Default::default() }, Asked::Message));
        self.update_hud();
        self.redraw();
    }

    /// View Tutorial: the original's opened its help, cut from the GOG
    /// build; here the help's tutorial topic (the first whose title has
    /// "Tutorial"), or the pointer to the manual.
    fn tutorial(&mut self) {
        let topic = self.help().and_then(|h| h.topics.iter().position(|t| t.title.contains("Tutorial")));
        match topic {
            Some(k) => self.open_topic(k),
            None => self.manual_note(),
        }
        self.update_hud();
        self.redraw();
    }

    /// Run one of the game's own programs in `DATA` (the historical video
    /// and the intro are Director projectors for Windows): directly on
    /// Windows, elsewhere through the command `GBOH_WINE` names, if set;
    /// else the pointer to the manual. The game does not wait for it.
    fn run_program(&mut self, name: &str) {
        let Some(path) = find_ci(&self.data_dir, name) else { return self.manual_note() };
        // the rewrite plays the movie inside itself
        if let Some(s) = slides::Slides::open(&path, &self.data_dir) {
            self.sound.set_music(sound::Music::Silent);
            if let Some(g) = &self.gpu {
                g.set_palette(&s.palette(&self.base_palette));
            }
            self.slides = Some(s);
            self.update_hud();
            self.redraw();
            return;
        }
        let wine = std::env::var_os("GBOH_WINE");
        let started = if cfg!(windows) {
            std::process::Command::new(&path).current_dir(&self.data_dir).spawn().is_ok()
        } else if let Some(w) = wine {
            std::process::Command::new(w).arg(&path).current_dir(&self.data_dir).spawn().is_ok()
        } else {
            false
        };
        if !started {
            self.manual_note();
        }
    }

    /// The intro: Alexander's Director projector, or Hannibal's and
    /// Caesar's `INTRO.AVI` beside `DATA`, over the whole window.
    fn play_intro(&mut self) {
        if find_ci(&self.data_dir, "INTRO.EXE").is_some() {
            self.run_program("INTRO.EXE");
            self.plain = None;
            return;
        }
        let avi = self.data_dir.parent().and_then(|p| find_ci(p, "INTRO.AVI"));
        if let Some(mut v) = avi.and_then(|p| video::Video::open_true_colour(&p, &self.base_palette)) {
            self.sound.set_music(sound::Music::Silent);
            if let (Some(g), Some(p)) = (&self.gpu, &v.palette) {
                g.set_palette(p);
            }
            if let Some(wav) = v.sound.take() {
                self.sound.play_data(wav);
            }
            self.video = Some(v);
            self.intro = true;
            self.update_hud();
            self.redraw();
        }
    }

    /// The intro over (or cut short): the start screen and its music.
    fn end_intro(&mut self) {
        self.intro = false;
        self.video = None;
        self.upload_palette();
        self.sound.stop_data();
        self.sound.set_music(sound::Music::Start);
        self.update_hud();
        self.redraw();
    }

    /// A testing click or drag (see `GBOH_CLICKS`), as the mouse makes it.
    fn script_step(&mut self) {
        let Some((from, to)) = self.script.pop_front() else { return };
        // (a click ends the intro, as the mouse's does)
        if self.slides.is_some() {
            self.end_slides();
            return;
        }
        if self.intro {
            self.end_intro();
            return;
        }
        let at = |(x, y): (f64, f64)| PhysicalPosition::new(x, y);
        self.cursor = at(from);
        if !self.map_drag(true) {
            self.set_pressed(true);
        }
        if let Some(to) = to {
            self.cursor = at(to);
            self.map_move();
        }
        if !self.map_drag(false) {
            self.set_pressed(false);
            self.click(false);
        }
        self.redraw();
    }

    /// Each level's background tile: the battle's (none over the setup and
    /// the rewrite's screens).
    fn tiles(&self) -> Vec<Option<Tile>> {
        if self.setup.is_some() || self.shell.is_some() || self.editor.is_some() {
            return Vec::new();
        }
        self.scenes.iter().map(|s| s.background.clone()).collect()
    }

    // --- the rewrite's own screens ---

    /// The window's title for the start screen: the game's own (string 1).
    fn hud_title(&self) -> String {
        crate::ui::exe_resources(&self.data_dir).and_then(|r| r.string(1)).unwrap_or_else(|| "opengboh".into())
    }

    // --- the campaign ---

    fn campaign_art(&mut self) -> &campaign::CampaignArt {
        if self.campaign_art.is_none() {
            self.campaign_art = Some(campaign::CampaignArt::new(&self.data_dir));
        }
        self.campaign_art.as_ref().unwrap()
    }

    /// The intro or historical video over (or cut short): the game's
    /// colours and the title music again.
    fn end_slides(&mut self) {
        self.slides = None;
        self.sound.channels_stop();
        self.upload_palette();
        if self.setup.is_some() {
            // the start screen fits the window from the first
            self.show_setup();
            self.sound.set_music(sound::Music::Start);
        }
        self.update_hud();
        self.redraw();
    }

    /// The victory video over: on to the results, and their music.
    fn close_video(&mut self) {
        self.video = None;
        self.plain = None;
        self.sound.stop_data();
        if let Some(r) = self.campaign_results {
            self.sound.set_music(sound::Music::End { won: r.victory });
        }
    }

    /// Typing in a dialog with edit boxes (save and load, the network
    /// ones, the chat); returns whether it took the key.
    fn plain_key(&mut self, key: &Key, text: Option<&str>) -> bool {
        let Some((d, asked)) = &mut self.plain else { return false };
        let Some(focus) = d.focus else { return false };
        let asked = asked.clone();
        match key {
            Key::Named(NamedKey::Escape) => self.plain = None,
            Key::Named(NamedKey::Enter) => {
                let d = d.clone();
                self.plain_ok(&d, &asked);
                return true;
            }
            Key::Named(NamedKey::Tab) => {
                // to the dialog's next edit box
                let edits: Vec<u16> = crate::ui::exe_resources(&self.data_dir)
                    .and_then(|r| r.dialog(d.name))
                    .map(|t| t.controls.iter().filter(|c| c.class == gboh_formats::pe::Name::Id(0x81)).map(|c| c.id).collect())
                    .unwrap_or_default();
                if let Some(k) = edits.iter().position(|&e| e == focus) {
                    d.focus = Some(edits[(k + 1) % edits.len()]);
                }
            }
            Key::Named(NamedKey::Backspace) => {
                if let Some((_, t)) = d.texts.iter_mut().find(|(k, _)| *k == focus) {
                    t.pop();
                }
            }
            _ => {
                let Some(typed) = text else { return true };
                // file names keep to letters, digits, spaces, - and _
                let file = matches!(asked, Asked::Files { .. }) && focus == 12;
                let ok = |c: char| !c.is_control() && (!file || c.is_ascii_alphanumeric() || " -_".contains(c));
                match d.texts.iter_mut().find(|(k, _)| *k == focus) {
                    Some((_, t)) => t.extend(typed.chars().filter(|&c| ok(c))),
                    None => d.texts.push((focus, typed.chars().filter(|&c| ok(c)).collect())),
                }
            }
        }
        self.update_hud();
        self.redraw();
        true
    }

    /// OK (or Enter) on a dialog with edit boxes.
    fn plain_ok(&mut self, d: &hud::Plain, asked: &Asked) {
        let get = |id: u16| d.texts.iter().find(|(k, _)| *k == id).map(|(_, t)| t.clone()).unwrap_or_default();
        match asked {
            Asked::Files { save, .. } => self.files_ok(*save, &get(12), &get(10)),
            Asked::Network(step) => self.network_ok(*step, d),
            Asked::Chat => {
                let line = get(107);
                self.plain = None;
                self.send_chat(&line);
            }
            _ => self.plain = None,
        }
        self.update_hud();
        self.redraw();
    }

    /// Finish the active leader: first asking whether he tries for
    /// momentum, if he may (the original's `MOMENTUMDLG`).
    fn finish_leader(&mut self) {
        if !self.my_turn() {
            return;
        }
        let Some(p) = &mut self.play else { return };
        if p.over {
            return;
        }
        // Finish ends the deployment phase
        if p.deploying {
            p.finish_deployment();
            self.changed();
            self.update_hud();
            self.redraw();
            return;
        }
        if p.game.can_attempt_momentum() {
            self.plain = Some((hud::Plain { name: "MOMENTUMDLG", ..Default::default() }, Asked::Momentum));
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
            let plain = |name: &'static str, texts: Vec<(u16, String)>| Some(hud::Plain { name, texts, ..Default::default() });
            let shown = match &notice {
                play::Notice::Momentum { leader, won } => plain(
                    "INITIATIVERESULTS",
                    vec![(101, format!("{} tried for momentum and {}", name(*leader), if *won { "goes again!" } else { "failed." })), (102, String::new())],
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
                play::Notice::Volley { .. } if self.prefs.quiet_missiles => None,
                play::Notice::Volley { target, hits, routed } => plain(
                    "MISSILERESULTS",
                    vec![
                        (101, format!("{} suffers {hits} cohesion hit{}", name(*target), if *hits == 1 { "" } else { "s" })),
                        (102, if *routed { "and is routing!".to_string() } else { "from the volley.".to_string() }),
                    ],
                ),
                play::Notice::NoOrders if !self.prefs.finish_leader_dialog || p.demo => None,
                play::Notice::NoOrders => plain("LEADERALMOSTFINISHED", Vec::new()),
                // (Caesar's is the same dialog under its own name)
                play::Notice::Elite { .. } => plain(
                    if gboh_formats::game::Game::detect(&self.data_dir) == Some(gboh_formats::game::Game::Caesar) {
                        "JCELITEINITDLG"
                    } else {
                        "HANELITEINITDLG"
                    },
                    Vec::new(),
                ),
            };
            if let Some(d) = shown {
                self.plain = Some((d, Asked::Notice(notice)));
                return;
            }
        }
    }

    fn upload_palette(&self) {
        // the rewrite's own screens keep their palette
        if self.shell.is_some() {
            if let Some(g) = &self.gpu {
                g.set_palette(&shell::palette());
            }
            return;
        }
        if let Some(g) = &self.gpu {
            let mut pal = self.base_palette.clone();
            let game = gboh_formats::game::Game::detect(&self.data_dir).unwrap_or_default();
            gboh_render::animate_water(&mut pal, self.water_frame, game);
            g.set_palette(&pal);
        }
    }

    fn redraw(&self) {
        if let Some(g) = &self.gpu {
            g.window.request_redraw();
        }
    }
}

/// The game's palette file (`GAME.PAL`, or the game's own name for it).
fn palette_path(dir: &Path) -> Option<PathBuf> {
    gboh_formats::pal::FILES.iter().find_map(|n| find_ci(dir, n))
}

fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok().map(|e| e.path())).find(|p| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(name)))
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

/// A testing click (from, no destination) or drag (from, to).
type ScriptStep = ((f64, f64), Option<(f64, f64)>);

/// One step of `GBOH_CLICKS`: "x,y" or "x,y>x2,y2".
fn script_step(step: &str) -> Option<ScriptStep> {
    let point = |p: &str| {
        let (x, y) = p.trim().split_once(',')?;
        Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
    };
    let mut parts = step.split('>');
    let from = point(parts.next()?)?;
    Some((from, parts.next().and_then(point)))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "-h" || a == "--help") {
        bail!("usage: opengboh  (the content manager, or the imported game's start screen)\n       opengboh <DATA dir> [MAP or SCN file] [ai|demo]");
    }
    // no folder given: the library's games, the selected game's start screen (or
    // the content manager while none is imported)
    let settings = library::Settings::load();
    let selected = if args.get(1).is_none() { settings.game_in_force(&library::imported()) } else { None };
    let launcher = args.get(1).is_none();
    let data_dir = match (args.get(1), selected) {
        (Some(d), _) => PathBuf::from(d),
        (None, Some(t)) => library::data_dir(t).unwrap_or_default(),
        (None, None) => PathBuf::new(),
    };
    let base_palette = match palette_path(&data_dir).and_then(|p| std::fs::read(p).ok()) {
        Some(data) => Palette::parse(&data)?,
        None if launcher => shell::palette(),
        None => bail!("GAME.PAL not found in {}", data_dir.display()),
    };
    // no file: the setup screens choose the battle; `ai` after the file:
    // the computer plays the visitor army with its first plan; `demo`: it
    // plays both, paced by the viewer
    let (setup, battle, battle_info) = match args.get(2) {
        None if launcher && selected.is_none() => (None, None, (String::new(), [false; 2])),
        None => (Some(setup::Setup::new(&data_dir)), None, (String::new(), [false; 2])),
        Some(file) => {
            let computer = match args.get(3).map(String::as_str) {
                Some("ai") => [false, true],
                Some("demo") => [true, true],
                _ => [false, false],
            };
            (None, Some(load_battle(&data_dir, file, computer, false)?), (file.clone(), computer))
        }
    };
    let (title, map_file, scenes, canvases, play) = match battle {
        Some(b) => (b.title, b.map_file, b.scenes, b.canvases, b.play),
        None => (String::from("opengboh"), String::new(), Vec::new(), vec![setup.as_ref().map_or_else(|| Canvas::new(1, 1, 0), |s| s.canvas())], None),
    };
    let level = if canvases.len() > 1 { 1 } else { 0 };
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
        battle: battle_info,
        autosaved: 0,
        prefs: prefs::Prefs::load(&prefs::Prefs::path()),
        menu_bar: menu::MenuBar::load(&data_dir, "CHILDMENU"),
        menu_open: None,
        show_commentary: true,
        save_name: None,
        sliding: None,
        reader: None,
        help: None,
        campaign: None,
        campaign_art: None,
        campaign_map: None,
        map_press: None,
        campaign_results: None,
        net: None,
        video: None,
        slides: None,
        intro: false,
        cursors: Default::default(),
        cursor_shown: None,
        edge: None,
        next_flag: Instant::now(),
        deploy_next: false,
        options_next: None,
        net_options: None,
        net_start: None,
        shell: (launcher && selected.is_none()).then_some(shell::Screen::Content),
        editor: None,
        shell_buttons: Vec::new(),
        shell_pressed: None,
        script: std::env::var("GBOH_CLICKS").unwrap_or_default().split(';').filter_map(script_step).collect(),
        script_next: Instant::now() + std::time::Duration::from_secs(3),
        news: (String::new(), false),
        importing: None,
        settings,
        game: selected,
        display_scale: 1.0,
        setup_offset: (0, 0),
        hover: None,
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
        modifiers: Default::default(),
    };
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut viewer)?;
    Ok(())
}

/// A battle ready to show: its title, scenes and canvases for the three
/// zoom levels, and the game (for a scenario).
/// A network game (see `net`): the connection (or the host still waiting
/// for one), the players' names, the battle the host chose while waiting, and how many
/// of the battle's orders have gone out.
struct NetGame {
    link: Option<net::Link>,
    waiting: Option<net::Waiting>,
    name: String,
    other: String,
    chosen: Option<(String, [u16; 2], [bool; 2])>,
    sent: usize,
    /// The armies (home, visitor) this machine leads.
    mine: [bool; 2],
}

/// The scenario nation byte of the Macedonians (the campaign's army).
const MACEDONIAN: u8 = 2;

/// The game's cursors (`alex.exe` cursor groups).
const CURSORS: [&str; 13] = [
    "SWORD",
    "SLING",
    "ARROW",
    "BOLT",
    "JAVELIN",
    "MOVENORTH",
    "MOVENORTHEAST",
    "MOVESOUTHEAST",
    "MOVESOUTH",
    "MOVESOUTHWEST",
    "MOVENORTHWEST",
    "MOVEEAST",
    "MOVEWEST",
];

/// What the Help menu shows in the text window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Info {
    About,
    Battle,
    Victory,
}

/// What a dialog popped up during play is for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    /// "Would you like to attempt momentum?"
    Momentum,
    /// Telling the player something.
    Notice(play::Notice),
    /// Confirming one of Hannibal's group commands.
    Group(play::GroupOrder),
    /// Saving (or loading) a game, with the saves found: file and
    /// description.
    Files { save: bool, entries: Vec<(String, String)> },
    /// The preferences.
    Preferences,
    /// A message: OK or Cancel closes it.
    Message,
    /// The network chat: a line to send.
    Chat,
    /// The victory video: Close.
    Video,
    /// More...: the player's scenario files to choose from.
    Custom { files: Vec<String> },
    /// A battle's variants: its standard scenario (none) or one of these.
    Variants { files: Vec<Option<String>> },
    /// A step of setting up a network game.
    Network(net::Step),
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
/// Taken by the tests that point `GBOH_SAVES` (the process's, not the
/// test's) at a folder of their own, so they run one at a time and none
/// writes into the real one.
#[cfg(test)]
static SAVES_VAR: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn load_battle(data_dir: &Path, file: &str, computer: [bool; 2], deploy: bool) -> Result<Battle> {
    // (the game's files, else the editor's scenarios, maps and plans)
    let load = |name: &str| find_ci(data_dir, name).or_else(|| find_ci(&editor::scenarios_dir(), name)).and_then(|p| std::fs::read(p).ok());
    // a scenario brings its map and units
    let scenario =
        if file.to_ascii_lowercase().ends_with(".scn") { Some(Scenario::parse(&load(file).with_context(|| format!("{file} not found"))?)?) } else { None };
    let map_file = scenario.as_ref().map_or_else(|| file.to_string(), |s| s.map_file.clone());
    let units = scenario.as_ref().map_or_else(Vec::new, |s| s.units.clone());
    let map_file = map_file.as_str();
    let map = Map::parse(&load(map_file).with_context(|| format!("{map_file} not found"))?)?;
    let flagged = scenario.as_ref().map_or_else(Vec::new, |s| gboh_game::command::opening_flags(s, &map, 1));
    let views: Vec<UnitView> = units.iter().enumerate().filter_map(|(i, u)| UnitView::of_scenario(&map, i, u)).collect();
    let every = scenario.as_ref().is_some_and(|s| s.layout == gboh_formats::scn::Layout::Editor);
    let scenes = build_scenes(data_dir, map_file, &map, false, every)?;
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
    let play = scenario.as_ref().map(|s| play::Play::start(gboh_game::game::Game::new(s, map.clone(), 1), ai, deploy));
    let title = if map.name.is_empty() { map_file.to_string() } else { map.name.clone() };
    Ok(Battle { title, map_file: map_file.to_string(), scenes, canvases, play })
}

/// The battlefield of `map` (loaded from `map_file`) at the three zooms,
/// reversed if `reversed`; with the game's unit types, or (`every`, a
/// *Collector's Edition* scenario, whose data has them all) every game's.
fn build_scenes(data_dir: &Path, map_file: &str, map: &Map, reversed: bool, every: bool) -> Result<Vec<Scene>> {
    let mut load = |name: &str| find_ci(data_dir, name).and_then(|p| std::fs::read(p).ok());
    let titles = eif_title_index(data_dir);
    let mut load_title = |title: &str| titles.get(title).and_then(|n| std::fs::read(data_dir.join(n)).ok());
    let palette = Palette::parse(&std::fs::read(palette_path(data_dir).context("GAME.PAL not found")?)?)?;
    let game = gboh_formats::game::Game::detect(data_dir).unwrap_or_default();
    let types: Vec<usize> = if every { (0..gboh_formats::units::TYPE_COUNT).collect() } else { gboh_formats::units::types_of(game).collect() };
    Ok(ZOOMS.iter().map(|&z| Scene::build_view(map_file, map, z, reversed, &palette, 1, &types, &mut load, &mut load_title)).collect::<Result<Vec<_>, _>>()?)
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
            let b = load_battle(Path::new(&dir), file, [false, true], false).unwrap_or_else(|e| panic!("{file}: {e:#}"));
            assert!(b.play.is_some(), "{file}");
            assert_eq!(b.canvases.len(), 3);
        }
    }
}
