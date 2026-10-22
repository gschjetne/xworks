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
//! AccessKit. Cut, Copy and Paste go through the system clipboard, as plain
//! text.
//!
//! - `XWORKS_ROOT`: where `spec/` and `fonts/` are; otherwise the nearest
//!   folder holding them, up from the program or the current folder.
//! - `XWORKS_DOCUMENTS`: where new documents go and Open looks; otherwise
//!   the Documents folder in the home folder.
//! - `XWORKS_WAYLAND`: under WSL, use Wayland rather than X11.
//! - `XWORKS_MEASURE_STARTUP`: for `tools/budgets`: once the first frame
//!   is presented, print "ready", the microseconds since starting, and
//!   each phase of the start as `phase=microseconds`, then quit when
//!   standard input closes.
//! - `WGPU_BACKEND`: vulkan, dx12, metal or gl, to pick a GPU backend;
//!   otherwise Vulkan, Metal or DirectX 12, whichever the system has.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use app::{Editor, Effect, Event, Resources};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy, OwnedDisplayHandle};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowId};

/// What arrives on the event loop from elsewhere.
enum UserEvent {
    /// A screen reader wants the tree, or wants something done.
    Access(accesskit_winit::Event),
    /// A renderer for frames of a format, its shaders compiled.
    Renderer(wgpu::TextureFormat, Box<render_gpu::Renderer>),
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
    /// The thread opening the GPU, until the first window joins it.
    opening: Option<JoinHandle<Gpu>>,
    instance: Option<wgpu::Instance>,
    gpu: Option<render_gpu::Device>,
    /// Whether the GPU is software, such as llvmpipe, so frames are drawn
    /// by render-cpu, which is faster on a CPU, and copied to the window.
    software: bool,
    renderers: HashMap<wgpu::TextureFormat, render_gpu::Renderer>,
    /// The formats whose renderers are compiling their shaders on a
    /// thread; render-cpu draws their frames meanwhile.
    compiling: HashSet<wgpu::TextureFormat>,
    windows: HashMap<WindowId, Win>,
    clipboard: Clipboard,
    modifiers: ModifiersState,
    start: Option<Option<PathBuf>>,
    proxy: EventLoopProxy<UserEvent>,
    /// The start's phases, if it's measuring them.
    measuring: Option<Measuring>,
    /// When the start began, while measuring waits for the GPU renderer's
    /// first frame.
    settling: Option<Instant>,
}

/// What the thread opening the GPU hands over.
struct Gpu {
    instance: wgpu::Instance,
    /// A device on the fastest adapter, if there's one.
    device: Option<render_gpu::Device>,
    /// How long the instance and the device took, for measuring.
    took: [(&'static str, Duration); 2],
}

/// Opens the GPU, on a thread of its own from the program's start, while
/// the fonts load and the window opens: an instance, and a device on the
/// fastest adapter, which a window's surface is checked against once there
/// is one. The backend is Vulkan, Metal or DirectX 12 unless
/// `WGPU_BACKEND` asks; on Windows, Vulkan first, which opens in a fifth
/// of the time, and DirectX 12 where there's no Vulkan.
fn open_gpu(display: OwnedDisplayHandle) -> Gpu {
    let started = Instant::now();
    let asked = std::env::var_os("WGPU_BACKEND").is_some();
    let instance = |backends| {
        let mut descriptor =
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display.clone()));
        if !asked {
            // Not OpenGL unless asked: starting it as well breaks the
            // Wayland connection under WSLg, even when Vulkan draws.
            descriptor.backends = backends;
        }
        wgpu::Instance::new(descriptor)
    };
    let windows = cfg!(target_os = "windows") && !asked;
    let mut made = instance(if windows {
        wgpu::Backends::VULKAN
    } else {
        wgpu::Backends::PRIMARY
    });
    let instanced = started.elapsed();
    let mut device = render_gpu::open(&made, None);
    if device.is_none() && windows {
        made = instance(wgpu::Backends::DX12);
        device = render_gpu::open(&made, None);
    }
    Gpu {
        instance: made,
        device,
        took: [
            ("gpu-instance", instanced),
            ("gpu-device", started.elapsed() - instanced),
        ],
    }
}

/// The clipboard, as plain text: the system's, opened at the first cut,
/// copy or paste so the start doesn't wait on it. Where it can't be
/// reached, cut and copied text stays within xWorks.
#[derive(Default)]
struct Clipboard {
    system: Option<arboard::Clipboard>,
    opened: bool,
    /// What xWorks last cut or copied.
    own: String,
}

impl Clipboard {
    fn system(&mut self) -> Option<&mut arboard::Clipboard> {
        if !self.opened {
            self.opened = true;
            match arboard::Clipboard::new() {
                Ok(system) => self.system = Some(system),
                Err(e) => eprintln!("xworks: the system clipboard: {e}"),
            }
        }
        self.system.as_mut()
    }

    fn copy(&mut self, text: String) {
        // Windows' programs expect lines to end in a carriage return and a
        // line feed.
        let lines = if cfg!(windows) {
            text.replace('\n', "\r\n")
        } else {
            text.clone()
        };
        if let Some(system) = self.system()
            && let Err(e) = system.set_text(lines)
        {
            eprintln!("xworks: copying to the system clipboard: {e}");
        }
        self.own = text;
    }

    /// The text to paste, if there is any: none when the clipboard holds
    /// something else, such as a picture.
    fn paste(&mut self) -> Option<String> {
        let Some(system) = self.system() else {
            return Some(self.own.clone());
        };
        match system.get_text() {
            Ok(text) => Some(text),
            Err(arboard::Error::ContentNotAvailable) => None,
            Err(e) => {
                eprintln!("xworks: pasting from the system clipboard: {e}");
                Some(self.own.clone())
            }
        }
    }
}

/// The phases of the start, timed for `tools/budgets`.
struct Measuring {
    started: Instant,
    last: Instant,
    phases: Vec<(&'static str, Duration)>,
}

impl Measuring {
    /// Ends `phase`, which began when the last one ended.
    fn mark(&mut self, phase: &'static str) {
        let now = Instant::now();
        self.phases.push((phase, now - self.last));
        self.last = now;
    }

    /// "ready", the time since starting, and the phases, in microseconds.
    fn report(&self) -> String {
        let mut line = format!("ready {}", self.started.elapsed().as_micros());
        for (phase, took) in &self.phases {
            line.push_str(&format!(" {phase}={}", took.as_micros()));
        }
        line
    }
}

/// Marks the end of a phase of the start, if it's being measured.
fn mark(measuring: &mut Option<Measuring>, phase: &'static str) {
    if let Some(m) = measuring {
        m.mark(phase);
    }
}

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    let mut measuring = std::env::var_os("XWORKS_MEASURE_STARTUP").map(|_| Measuring {
        started,
        last: started,
        phases: Vec::new(),
    });
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
    let display = event_loop.owned_display_handle();
    let opening = std::thread::spawn(move || open_gpu(display));
    mark(&mut measuring, "event-loop");
    let root = root().context("can't find xWorks' spec/ and fonts/; set XWORKS_ROOT")?;
    let documents = documents();
    let mac = cfg!(target_os = "macos");
    let res =
        Resources::load(&root, documents, mac).context("loading fonts, styles and commands")?;
    mark(&mut measuring, "resources");
    let mut shell = Shell {
        res: Arc::new(res),
        opening: Some(opening),
        instance: None,
        gpu: None,
        software: false,
        renderers: HashMap::new(),
        compiling: HashSet::new(),
        windows: HashMap::new(),
        clipboard: Clipboard::default(),
        modifiers: ModifiersState::empty(),
        start: Some(start),
        proxy: event_loop.create_proxy(),
        measuring,
        settling: None,
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

/// Shows `path` in Explorer, selected.
#[cfg(target_os = "windows")]
fn reveal(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    // Explorer reads its own command line: the path is quoted after the
    // comma, where quoting the whole argument would hide it.
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(drop)
}

/// Shows `path` in the Finder, selected.
#[cfg(target_os = "macos")]
fn reveal(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(drop)
}

/// Shows the folder `path` is in.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn reveal(path: &Path) -> std::io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("."));
    std::process::Command::new("xdg-open")
        .arg(folder)
        .spawn()
        .map(drop)
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
        mark(&mut self.measuring, "document");
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
        mark(&mut self.measuring, "window");
        if let Some(opening) = self.opening.take() {
            let opened = opening
                .join()
                .map_err(|_| anyhow::anyhow!("opening the GPU failed"))?;
            self.instance = Some(opened.instance);
            self.gpu = opened.device;
            if let Some(m) = &mut self.measuring {
                m.mark("gpu");
                m.phases.extend(opened.took);
            }
        }
        let instance = self.instance.as_ref().context("no GPU instance")?;
        let surface = instance.create_surface(window.clone())?;
        mark(&mut self.measuring, "surface");
        // The GPU was opened without a window. Where its adapter can't
        // draw on this one, as on a laptop whose display hangs off its
        // other GPU, one that can is opened.
        if !self
            .gpu
            .as_ref()
            .is_some_and(|g| g.adapter.is_surface_supported(&surface))
        {
            self.gpu = render_gpu::open(instance, Some(&surface));
            self.renderers.clear();
            mark(&mut self.measuring, "adapter");
        }
        self.software = self
            .gpu
            .as_ref()
            .is_some_and(|g| g.adapter.get_info().device_type == wgpu::DeviceType::Cpu);
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
        // render-cpu's frames are copied in: on a software GPU, and on
        // others until the GPU renderer is ready, where the surface allows.
        let config = wgpu::SurfaceConfiguration {
            usage: if self.software {
                wgpu::TextureUsages::COPY_DST
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | (caps.usages & wgpu::TextureUsages::COPY_DST)
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
        // render-cpu draws for a software GPU, so it needs no renderer:
        // making one compiles shaders that llvmpipe would run. On others,
        // vello_gpu compiles its shaders on a thread, which takes a quarter
        // of a second, and render-cpu draws the frames until it's done.
        if !self.software && !self.renderers.contains_key(&format) {
            if !config.usage.contains(wgpu::TextureUsages::COPY_DST) {
                let renderer = render_gpu::Renderer::new(&gpu.device, format);
                self.renderers.insert(format, renderer);
            } else if self.compiling.insert(format) {
                let (device, proxy) = (gpu.device.clone(), self.proxy.clone());
                std::thread::spawn(move || {
                    let renderer = Box::new(render_gpu::Renderer::new(&device, format));
                    let _ = proxy.send_event(UserEvent::Renderer(format, renderer));
                });
            }
        }
        mark(&mut self.measuring, "renderer");
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
        let renderer = match self.software {
            true => None,
            false => self.renderers.get_mut(&win.config.format),
        };
        let drawn_by_gpu = renderer.is_some();
        match renderer {
            Some(renderer) => {
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let desk = self.res.theme.desk;
                if let Err(e) = renderer.render(gpu, &page, ppi, &view, w, h, desk) {
                    eprintln!("xworks: drawing the window: {e}");
                }
            }
            None => upload(gpu, &frame.texture, &page, ppi, win.config.format),
        }
        win.window.pre_present_notify();
        gpu.queue.present(frame);
        if drawn_by_gpu && let Some(started) = self.settling.take() {
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            println!("settled {}", started.elapsed().as_micros());
        }
        if let Some(mut measuring) = self.measuring.take() {
            // The first frame is on screen once the GPU has done it.
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            measuring.mark("frame");
            println!("{}", measuring.report());
            // Memory is measured once the renderer that stays draws.
            if drawn_by_gpu || self.software {
                println!("settled {}", measuring.started.elapsed().as_micros());
            } else {
                self.settling = Some(measuring.started);
            }
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
                Effect::Copy(text) => self.clipboard.copy(text),
                Effect::Paste => {
                    if let Some(text) = self.clipboard.paste() {
                        effects.extend(win.editor.update(Event::Pasted(text)));
                    }
                }
                Effect::Open(path) => opening.push(path),
                Effect::OpenExternally(path) => {
                    if let Err(e) = open_externally(&path) {
                        eprintln!("xworks: opening {}: {e}", path.display());
                    }
                }
                Effect::Reveal(path) => {
                    if let Err(e) = reveal(&path) {
                        eprintln!("xworks: showing {}: {e}", path.display());
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
            UserEvent::Renderer(format, renderer) => {
                self.compiling.remove(&format);
                self.renderers.insert(format, *renderer);
                for win in self.windows.values().filter(|w| w.config.format == format) {
                    win.window.request_redraw();
                }
                return;
            }
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
