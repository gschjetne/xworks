//! The generic shell: xWorks on winit and wgpu, with its own toolkit drawing
//! the chrome (docs/ARCHITECTURE.md, section 8).
//!
//! ```text
//! xworks [FILE.xw]      open a document, or make a new one
//! ```
//!
//! Each window holds one [`app::Editor`]: the shell turns winit's events into
//! the editor's, carries out the effects it returns, and draws its view with
//! render-gpu. It also feeds the input method and screen readers, through
//! AccessKit. Cut and copied text stays within xWorks for now.
//!
//! - `XWORKS_ROOT`: where `spec/` and `fonts/` are; otherwise the nearest
//!   folder holding them, up from the program or the current folder.
//! - `XWORKS_DOCUMENTS`: where new documents go and Open looks; otherwise
//!   the Documents folder in the home folder.
//! - `XWORKS_WAYLAND`: under WSL, use Wayland rather than X11.
//! - `XWORKS_MEASURE_STARTUP`: for `tools/budgets`: once the first frame
//!   is presented, print "ready" and the time since starting, then quit
//!   when standard input closes.
//! - `WGPU_BACKEND`: vulkan, dx12, metal or gl, to pick a GPU backend;
//!   otherwise Vulkan, Metal or DirectX 12, whichever the system has.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use app::{Editor, Effect, Event, Resources};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowId};

/// The desk behind the pages, which clears each frame.
const DESK: scene::Color = scene::Color::rgb(200, 201, 204);

/// What arrives on the event loop from elsewhere.
#[derive(Debug)]
enum UserEvent {
    /// A screen reader wants the tree, or wants something done.
    Access(accesskit_winit::Event),
    /// Measuring is done: standard input closed.
    Measured,
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(event: accesskit_winit::Event) -> UserEvent {
        UserEvent::Access(event)
    }
}

/// One window and its document.
struct Win {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    editor: Editor,
    access: accesskit_winit::Adapter,
    pointer: (f64, f64),
}

struct Shell {
    res: Arc<Resources>,
    instance: wgpu::Instance,
    gpu: Option<render_gpu::Device>,
    /// Whether the GPU is software, such as llvmpipe, so frames are drawn
    /// by render-cpu, which is faster on a CPU, and copied to the window.
    software: bool,
    renderers: HashMap<wgpu::TextureFormat, render_gpu::Renderer>,
    windows: HashMap<WindowId, Win>,
    /// Cut and copied text, within xWorks: the system clipboard waits on
    /// the project owner's decision on its Windows crate's license.
    clipboard: String,
    modifiers: ModifiersState,
    start: Option<Option<PathBuf>>,
    proxy: EventLoopProxy<UserEvent>,
    /// When the program started, if it's measuring its start.
    measuring: Option<std::time::Instant>,
}

fn main() -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let root = root().context("can't find xWorks' spec/ and fonts/; set XWORKS_ROOT")?;
    let documents = documents();
    let mac = cfg!(target_os = "macos");
    let res =
        Resources::load(&root, documents, mac).context("loading fonts, styles and commands")?;
    let start = std::env::args_os().nth(1).map(PathBuf::from);
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "linux")]
    if std::env::var_os("WSL_DISTRO_NAME").is_some() && std::env::var_os("XWORKS_WAYLAND").is_none()
    {
        // Under WSL, presenting frames through Mesa's software Vulkan on
        // WSLg's Wayland drops the connection now and then; X11 doesn't.
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    let event_loop = builder.build()?;
    let display = Box::new(event_loop.owned_display_handle());
    let mut descriptor = wgpu::InstanceDescriptor::new_with_display_handle_from_env(display);
    if std::env::var_os("WGPU_BACKEND").is_none() {
        // Vulkan, Metal or DirectX 12 unless asked: starting OpenGL as well
        // breaks the Wayland connection under WSLg, even when Vulkan draws.
        descriptor.backends = wgpu::Backends::PRIMARY;
    }
    let mut shell = Shell {
        res: Arc::new(res),
        instance: wgpu::Instance::new(descriptor),
        gpu: None,
        software: false,
        renderers: HashMap::new(),
        windows: HashMap::new(),
        clipboard: String::new(),
        modifiers: ModifiersState::empty(),
        start: Some(start),
        proxy: event_loop.create_proxy(),
        measuring: std::env::var_os("XWORKS_MEASURE_STARTUP").map(|_| started),
    };
    event_loop.run_app(&mut shell)?;
    Ok(())
}

/// Where `spec/` and `fonts/` are.
fn root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("XWORKS_ROOT") {
        return Some(PathBuf::from(root));
    }
    let has = |dir: &Path| dir.join("spec/text/fonts.ron").is_file();
    let from_exe = std::env::current_exe().ok();
    let from_cwd = std::env::current_dir().ok();
    [from_exe, from_cwd]
        .into_iter()
        .flatten()
        .find_map(|start| start.ancestors().find(|d| has(d)).map(Path::to_path_buf))
}

/// Where documents go.
fn documents() -> PathBuf {
    if let Some(dir) = std::env::var_os("XWORKS_DOCUMENTS") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join("Documents")
}

/// The editor's key for a winit named key, if it has one.
fn named(key: &NamedKey) -> Option<chrome::Key> {
    Some(match key {
        NamedKey::Enter => chrome::Key::Enter,
        NamedKey::Tab => chrome::Key::Tab,
        NamedKey::Backspace => chrome::Key::Backspace,
        NamedKey::Delete => chrome::Key::Delete,
        NamedKey::Escape => chrome::Key::Escape,
        NamedKey::ArrowLeft => chrome::Key::Left,
        NamedKey::ArrowRight => chrome::Key::Right,
        NamedKey::ArrowUp => chrome::Key::Up,
        NamedKey::ArrowDown => chrome::Key::Down,
        NamedKey::Home => chrome::Key::Home,
        NamedKey::End => chrome::Key::End,
        NamedKey::PageUp => chrome::Key::PageUp,
        NamedKey::PageDown => chrome::Key::PageDown,
        _ => return None,
    })
}

/// The document as a screen reader sees it: a window holding the
/// document's text.
fn tree(editor: &Editor) -> accesskit::TreeUpdate {
    use accesskit::{Node, NodeId, Role, TreeInfo, TreeUpdate};
    let (text, _, _) = editor.text();
    let mut window = Node::new(Role::Window);
    window.set_label(format!("{} — xWorks", editor.title()));
    window.set_children([NodeId(1)]);
    let mut document = Node::new(Role::Document);
    document.set_label(editor.title());
    document.set_value(text);
    TreeUpdate {
        nodes: vec![(NodeId(0), window), (NodeId(1), document)],
        tree: Some(TreeInfo::new(NodeId(0))),
        tree_id: accesskit::TreeId::ROOT,
        focus: NodeId(1),
    }
}

/// Draws `page` with render-cpu and copies it into `texture`, for a GPU
/// that's software.
fn upload(
    gpu: &render_gpu::Device,
    texture: &wgpu::Texture,
    page: &scene::Page,
    ppi: f64,
    format: wgpu::TextureFormat,
) {
    let image = render_cpu::render(page, ppi, render_cpu::Level::Fastest);
    let (w, h) = (
        image.width.min(texture.width()),
        image.height.min(texture.height()),
    );
    let bgra = matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for row in image
        .pixels
        .chunks_exact(image.width as usize * 4)
        .take(h as usize)
    {
        for &[r, g, b, a] in row[..w as usize * 4].as_chunks::<4>().0 {
            pixels.extend_from_slice(&if bgra { [b, g, r, a] } else { [r, g, b, a] });
        }
    }
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}

/// Hands `path` to the system to open with whatever opens it.
fn open_externally(path: &Path) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(path).spawn().map(drop)
}

impl Shell {
    /// Opens a window on a new document, or the one at `path`.
    fn open(&mut self, event_loop: &ActiveEventLoop, path: Option<PathBuf>) -> anyhow::Result<()> {
        let editor = match &path {
            Some(path) => Editor::open(self.res.clone(), path),
            None => Editor::create(self.res.clone()),
        }
        .with_context(|| match &path {
            Some(path) => format!("opening {}", path.display()),
            None => "making a new document".to_string(),
        })?;
        let attributes = Window::default_attributes()
            .with_title(format!("{} — xWorks", editor.title()))
            .with_inner_size(LogicalSize::new(1000.0, 820.0))
            .with_visible(false);
        let window = Arc::new(event_loop.create_window(attributes)?);
        let access = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        window.set_visible(true);
        window.set_ime_allowed(true);
        let surface = self.instance.create_surface(window.clone())?;
        if self.gpu.is_none() {
            self.gpu = render_gpu::open(&self.instance, Some(&surface));
            self.software = self
                .gpu
                .as_ref()
                .is_some_and(|g| g.adapter.get_info().device_type == wgpu::DeviceType::Cpu);
        }
        let Some(gpu) = &self.gpu else {
            bail!("no GPU can draw this window");
        };
        let caps = surface.get_capabilities(&gpu.adapter);
        // vello_gpu writes sRGB values, so a format that doesn't encode
        // them again.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .context("the surface offers no format")?;
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: if self.software {
                wgpu::TextureUsages::COPY_DST
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            },
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            color_space: wgpu::SurfaceColorSpace::Auto,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: Vec::new(),
        };
        surface.configure(&gpu.device, &config);
        self.renderers
            .entry(format)
            .or_insert_with(|| render_gpu::Renderer::new(&gpu.device, format));
        let mut win = Win {
            window,
            surface,
            config,
            editor,
            access,
            pointer: (0.0, 0.0),
        };
        let logical = win
            .window
            .inner_size()
            .to_logical::<f64>(win.window.scale_factor());
        win.editor.update(Event::Resize {
            width: logical.width,
            height: logical.height,
        });
        win.window.request_redraw();
        self.windows.insert(win.window.id(), win);
        Ok(())
    }

    fn draw(&mut self, id: WindowId) {
        let (Some(gpu), Some(win)) = (&self.gpu, self.windows.get_mut(&id)) else {
            return;
        };
        let frame = match win.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => {
                win.surface.configure(&gpu.device, &win.config);
                win.window.request_redraw();
                return;
            }
        };
        let page = win.editor.view();
        let ppi = 96.0 * win.window.scale_factor();
        let (w, h) = (
            win.config.width.min(65_535) as u16,
            win.config.height.min(65_535) as u16,
        );
        if self.software {
            upload(gpu, &frame.texture, &page, ppi, win.config.format);
        } else {
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            if let Some(renderer) = self.renderers.get_mut(&win.config.format)
                && let Err(e) = renderer.render(gpu, &page, ppi, &view, w, h, DESK)
            {
                eprintln!("xworks: drawing the window: {e}");
            }
        }
        win.window.pre_present_notify();
        gpu.queue.present(frame);
        if let Some(started) = self.measuring.take() {
            // The first frame is on screen once the GPU has done it.
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            println!("ready {}", started.elapsed().as_micros());
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                let mut rest = String::new();
                let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut rest);
                let _ = proxy.send_event(UserEvent::Measured);
            });
        }
    }

    /// Feeds `event` to window `id`'s editor and carries out what it asks.
    fn feed(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: Event) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        let mut effects = win.editor.update(event);
        let mut opening = Vec::new();
        while let Some(effect) = effects.pop() {
            match effect {
                Effect::Copy(text) => self.clipboard = text,
                Effect::Paste => {
                    let text = self.clipboard.clone();
                    effects.extend(win.editor.update(Event::Pasted(text)));
                }
                Effect::Open(path) => opening.push(path),
                Effect::OpenExternally(path) => {
                    if let Err(e) = open_externally(&path) {
                        eprintln!("xworks: opening {}: {e}", path.display());
                    }
                }
                Effect::Quit => event_loop.exit(),
            }
        }
        // The input method's window goes below the caret, and screen
        // readers see the text as it is.
        let (x, y, w, h) = win.editor.ime_area();
        win.window.set_ime_cursor_area(
            LogicalPosition::new(x, y),
            LogicalSize::new(w.max(1.0), h.max(1.0)),
        );
        let editor = &win.editor;
        win.access.update_if_active(|| tree(editor));
        win.window.request_redraw();
        for path in opening {
            if let Err(e) = self.open(event_loop, path) {
                eprintln!("xworks: {e:#}");
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(start) = self.start.take()
            && let Err(e) = self.open(event_loop, start)
        {
            eprintln!("xworks: {e:#}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let event = match event {
            UserEvent::Access(event) => event,
            UserEvent::Measured => return event_loop.exit(),
        };
        let Some(win) = self.windows.get_mut(&event.window_id) else {
            return;
        };
        if let accesskit_winit::WindowEvent::InitialTreeRequested = event.window_event {
            let editor = &win.editor;
            win.access.update_if_active(|| tree(editor));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        win.access.process_event(&win.window, &event);
        let scale = win.window.scale_factor();
        match event {
            WindowEvent::CloseRequested => {
                self.windows.remove(&id);
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &self.gpu {
                    win.config.width = size.width.max(1);
                    win.config.height = size.height.max(1);
                    win.surface.configure(&gpu.device, &win.config);
                }
                let logical = size.to_logical::<f64>(scale);
                self.feed(
                    event_loop,
                    id,
                    Event::Resize {
                        width: logical.width,
                        height: logical.height,
                    },
                );
            }
            WindowEvent::RedrawRequested => self.draw(id),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed => {
                let m = self.modifiers;
                let modifiers = chrome::Modifiers {
                    primary: if cfg!(target_os = "macos") {
                        m.super_key()
                    } else {
                        m.control_key()
                    },
                    shift: m.shift_key(),
                    alt: m.alt_key(),
                };
                let typed = match &event.logical_key {
                    Key::Named(key) => named(key).map(|key| Event::Key { key, modifiers }),
                    _ => None,
                };
                let typed = typed.or_else(|| {
                    if modifiers.primary {
                        // A shortcut: the key as it is without Shift, so
                        // shifted layouts don't change it.
                        match event.key_without_modifiers() {
                            Key::Character(c) => c.chars().next().map(|c| Event::Key {
                                key: chrome::Key::Char(c.to_ascii_lowercase()),
                                modifiers,
                            }),
                            _ => None,
                        }
                    } else {
                        event
                            .text
                            .as_ref()
                            .filter(|t| !t.chars().any(char::is_control))
                            .map(|t| Event::Text(t.to_string()))
                    }
                });
                if let Some(typed) = typed {
                    self.feed(event_loop, id, typed);
                }
            }
            WindowEvent::Ime(Ime::Preedit(text, _)) => {
                self.feed(event_loop, id, Event::Preedit(text))
            }
            WindowEvent::Ime(Ime::Commit(text)) => self.feed(event_loop, id, Event::Text(text)),
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f64>(scale);
                win.pointer = (p.x, p.y);
                self.feed(event_loop, id, Event::Move { x: p.x, y: p.y });
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = win.pointer;
                let event = match state {
                    ElementState::Pressed => Event::Press {
                        x,
                        y,
                        shift: self.modifiers.shift_key(),
                    },
                    ElementState::Released => Event::Release,
                };
                self.feed(event_loop, id, event);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -f64::from(y) * 48.0,
                    MouseScrollDelta::PixelDelta(p) => -p.to_logical::<f64>(scale).y,
                };
                self.feed(event_loop, id, Event::Scroll(dy));
            }
            WindowEvent::ScaleFactorChanged { .. } => win.window.request_redraw(),
            _ => {}
        }
    }
}
