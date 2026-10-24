//! The generic shell: xWorks on winit and wgpu, with its own toolkit drawing
//! the chrome (docs/ARCHITECTURE.md, section 8).
//!
//! ```text
//! xworks [FILE.xw]      open a document, or make a new one
//! xworks FILE.md        open a Markdown file, which saves as itself
//! ```
//!
//! Each window holds one [`app::Window`]: the shell turns winit's events into
//! the window's, carries out the effects it returns, and draws its view with
//! render-gpu, or render-cpu on a software GPU and while vello_gpu compiles
//! its shaders. A window keeps a canvas of its own, which each frame draws
//! into only where the view's display list changed and copies whole to the
//! window. It also feeds the input method and screen readers, through
//! AccessKit. Cut, Copy and Paste go through the system clipboard, as plain
//! text, and files are chosen in the system's own dialogs (rfd: portals on
//! Linux, the common dialogs on Windows; review a23vvez8). The caret blinks
//! at the system's rate (review ydqhfds0), read from its settings on a
//! thread of its own while the window opens; the shell wakes for each
//! blink, and a blink draws only the caret's pixels again.
//!
//! xWorks draws its own title bar (review 3ce247c2), so windows have no
//! decorations: the app asks for the window to be moved, resized from its
//! edges, minimised, maximised and closed, and the shell asks winit. Its
//! look is light or dark as the system's is.
//!
//! - `XWORKS_ROOT`: where `spec/` and `fonts/` are; otherwise the nearest
//!   folder holding them, up from the program or the current folder.
//! - `XWORKS_DOCUMENTS`: where new documents go and Open looks first;
//!   otherwise the Documents folder in the home folder.
//! - `XWORKS_THEME`: `dark` or `light`, where the system doesn't say, as
//!   under X11.
//! - `XWORKS_WAYLAND`: under WSL, use Wayland rather than X11.
//! - `XWORKS_MEASURE_STARTUP`: for `tools/budgets`: once the first frame
//!   is presented, print "ready", the microseconds since starting, and
//!   each phase of the start as `phase=microseconds`, then quit when
//!   standard input closes.
//! - `WGPU_BACKEND`: vulkan, dx12, metal or gl, to pick a GPU backend;
//!   otherwise Vulkan, Metal or DirectX 12, whichever the system has.

mod system;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use app::recorder::{self, Recorder, Recordings};
use app::{Choice, Effect, Event, Resources};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{
    ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle,
};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{CursorIcon, ResizeDirection, Theme, Window, WindowId};

/// What arrives on the event loop from elsewhere.
enum UserEvent {
    /// A screen reader wants the tree, or wants something done.
    Access(accesskit_winit::Event),
    /// A renderer for frames of a format, its shaders compiled.
    Renderer(wgpu::TextureFormat, Box<render_gpu::Renderer>),
    /// Measuring is done: standard input closed.
    Measured,
    /// A file chosen in the system's dialog for a window, or none.
    Chosen(WindowId, Choice, Option<PathBuf>),
    /// How the system has the caret blink, read from its settings.
    Blinking(app::Blinking),
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
    /// The document and its chrome.
    doc: app::Window,
    /// The title the system shows, as last set.
    title: String,
    access: accesskit_winit::Adapter,
    pointer: (f64, f64),
    /// The window's own picture, drawn where the view changed and copied
    /// to each frame, where the surface takes copies.
    canvas: Option<render_gpu::Canvas>,
    /// Whether the GPU renderer drew what the canvas shows, rather than
    /// render-cpu.
    canvas_by_gpu: bool,
    /// render-cpu, with the images it keeps, for a software GPU and the
    /// first frames.
    cpu: render_cpu::Renderer,
    /// The last press of the pointer: when, where, and its count in its
    /// run of quick presses in one place, for double and triple clicks,
    /// which winit doesn't count.
    pressed: Option<(Instant, (f64, f64), u32)>,
    /// Whether a file dialog is open for it, so another isn't.
    choosing: bool,
    /// Its caret's blinking, which it does while it has the keyboard.
    blink: app::Blink,
    /// Whether it has the keyboard.
    focused: bool,
}

/// How soon after a press another in the same place counts as the next of
/// a double or triple click: Windows' default, which winit can't ask the
/// system for.
const MULTI_CLICK_TIME: Duration = Duration::from_millis(500);
/// How far apart, in logical pixels, presses in the same place may be:
/// Windows' default.
const MULTI_CLICK_DISTANCE: f64 = 4.0;

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
    /// Whether Alt is down with no other key pressed since: let go, it
    /// goes to the menus on Windows and Linux.
    alt_alone: bool,
    start: Option<Option<PathBuf>>,
    proxy: EventLoopProxy<UserEvent>,
    /// The start's phases, if it's measuring them.
    measuring: Option<Measuring>,
    /// When the start began, while measuring waits for the GPU renderer's
    /// first frame.
    settling: Option<Instant>,
    /// Every window's recording, written when xWorks fails.
    recordings: Recordings,
    /// How many times the person's settings had changed when every
    /// window last drew them.
    settings_seen: u64,
    /// When the shell started, which blinks are timed from.
    started: Instant,
    /// How the system has the caret blink.
    blinking: app::Blinking,
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
    let mut res =
        Resources::load(&root, documents, mac).context("loading fonts, styles and commands")?;
    res.language = system_language();
    if let Some(config) = config_folder() {
        res.recent = Arc::new(app::Recent::load(config.join("recent.ron")));
        res.settings = Arc::new(app::Settings::load(
            config.join("settings.ron"),
            system::units(),
        ));
        // The person's own stylesheets, beside the settings (M2's stage 9).
        res.own = Arc::new(app::OwnSheets::load(config.join("stylesheets")));
    }
    let recordings = Recordings::default();
    write_when_failing(
        recordings.clone(),
        recorder::folder(&res.documents),
        res.recent.clone(),
    );
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
        alt_alone: false,
        start: Some(start),
        proxy: event_loop.create_proxy(),
        measuring,
        settling: None,
        recordings,
        settings_seen: 0,
        started,
        blinking: app::Blinking::At(app::BlinkRate::DEFAULT),
    };
    // The system's blink rate, read while the window opens: it may wait on
    // a program, gsettings or defaults. The caret shows steadily for a
    // pause at first, so it's there before the first blink.
    let proxy = shell.proxy.clone();
    std::thread::spawn(move || {
        let _ = proxy.send_event(UserEvent::Blinking(system::blinking()));
    });
    event_loop.run_app(&mut shell)?;
    // The documents opened lately, written to their file.
    shell.res.recent.flush();
    Ok(())
}

/// When xWorks fails, writes what was done in each window over the last
/// few minutes, as sessions for a bug report, to `folder`, which is in the
/// documents folder, and says where (project owner, 2026-10-23, review
/// a23vvez8). Nothing is sent anywhere. The documents opened lately are
/// written to their file too.
fn write_when_failing(recordings: Recordings, folder: PathBuf, recent: Arc<app::Recent>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        recent.flush();
        for path in recordings.write_all(&folder, &format!("xWorks failed: {info}")) {
            eprintln!(
                "xworks: what was done in the last few minutes is in {}",
                path.display()
            );
        }
    }));
}

/// The system's language, which new documents are in (project owner,
/// 2026-10-22, review pysc65hp): the first of the person's languages that
/// names one, or none, as for the C locale.
fn system_language() -> text::Language {
    sys_locale::get_locales()
        .filter(|tag| !matches!(tag.as_str(), "C" | "POSIX") && !tag.starts_with("C."))
        .find_map(|tag| text::Language::parse(&tag).ok())
        .unwrap_or_default()
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

/// Where xWorks keeps what it remembers between runs, such as the
/// documents opened lately: the system's place for programs' settings.
fn config_folder() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("xworks"))
}

/// Whether the look is dark: as `XWORKS_THEME` says, or the window's
/// system, or light where it doesn't say.
fn dark(window: &Window) -> bool {
    match std::env::var("XWORKS_THEME").as_deref() {
        Ok("dark") => true,
        Ok("light") => false,
        _ => window.theme() == Some(Theme::Dark),
    }
}

/// winit's direction for resizing from `edge`.
fn direction(edge: app::Edge) -> ResizeDirection {
    match edge {
        app::Edge::North => ResizeDirection::North,
        app::Edge::South => ResizeDirection::South,
        app::Edge::East => ResizeDirection::East,
        app::Edge::West => ResizeDirection::West,
        app::Edge::NorthEast => ResizeDirection::NorthEast,
        app::Edge::NorthWest => ResizeDirection::NorthWest,
        app::Edge::SouthEast => ResizeDirection::SouthEast,
        app::Edge::SouthWest => ResizeDirection::SouthWest,
    }
}

/// Asks the person for a file for `choice`, in the system's own dialog
/// over `window`, on a thread of its own so the window keeps drawing; the
/// answer comes back to the event loop.
fn choose(
    window: &Arc<Window>,
    choice: Choice,
    proxy: EventLoopProxy<UserEvent>,
    folder: &Path,
    name: &str,
    markdown: bool,
) {
    let dialog = rfd::FileDialog::new().set_parent(&**window);
    let dialog = match choice {
        // xWorks documents and Markdown files, which open as themselves
        // (M2's stage 6).
        Choice::Open => dialog
            .set_title("Open")
            .add_filter("Documents", &["xw", "md", "markdown"])
            .add_filter("xWorks documents", &["xw"])
            .add_filter("Markdown", &["md", "markdown"])
            .set_directory(folder),
        Choice::MoveTo if markdown => dialog
            .set_title("Move To")
            .add_filter("Markdown", &["md", "markdown"])
            .set_directory(folder)
            .set_file_name(format!("{name}.md")),
        Choice::MoveTo => dialog
            .set_title("Move To")
            .add_filter("xWorks documents", &["xw"])
            .set_directory(folder)
            .set_file_name(format!("{name}.xw")),
        Choice::Picture => dialog
            .set_title("Place a Picture")
            .add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "webp"]),
    };
    let id = window.id();
    std::thread::spawn(move || {
        let chosen = match choice {
            Choice::MoveTo => dialog.save_file(),
            Choice::Open | Choice::Picture => dialog.pick_file(),
        };
        let _ = proxy.send_event(UserEvent::Chosen(id, choice, chosen));
    });
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
        NamedKey::F10 => chrome::Key::F10,
        _ => return None,
    })
}

/// The window as a screen reader sees it, in physical pixels.
fn tree(doc: &app::Window, scale: f64) -> accesskit::TreeUpdate {
    let mut tree = doc.access();
    let root = tree.tree.as_ref().map(|t| t.root);
    for (id, node) in &mut tree.nodes {
        if Some(*id) == root {
            node.set_transform(accesskit::Affine::scale(scale));
        }
    }
    tree
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

/// Whether `a` and `b` name the same file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

impl Shell {
    /// Opens a window on a new document, or the one at `path`, or its
    /// named `version`. A document is never open twice (project owner,
    /// 2026-10-23, review a23vvez8): if a window has it, or has that
    /// version, that window comes forward instead.
    fn open(
        &mut self,
        event_loop: &ActiveEventLoop,
        path: Option<PathBuf>,
        version: Option<i64>,
    ) -> anyhow::Result<()> {
        if let Some(path) = &path
            && let Some(win) = self.windows.values().find(|w| {
                let editor = w.doc.editor();
                editor.version().map(|v| v.seq) == version && same_file(editor.path(), path)
            })
        {
            win.window.focus_window();
            return Ok(());
        }
        let mut doc = match (&path, version) {
            (Some(path), Some(seq)) => app::Window::open_version(self.res.clone(), path, seq),
            (Some(path), None) => app::Window::open(self.res.clone(), path),
            (None, _) => app::Window::create(self.res.clone()),
        }
        .with_context(|| match &path {
            Some(path) => format!("opening {}", path.display()),
            None => "making a new document".to_string(),
        })?;
        mark(&mut self.measuring, "document");
        // The last few minutes of what's done in it are kept, to be written
        // when asked or when xWorks fails.
        let recorder = Recorder::new(&self.res.recorder);
        doc.record(recorder.clone());
        self.recordings.add(&recorder);
        // xWorks draws its own title bar (review 3ce247c2).
        let attributes = Window::default_attributes()
            .with_title(doc.title())
            .with_inner_size(LogicalSize::new(1000.0, 820.0))
            .with_decorations(false)
            .with_visible(false);
        #[cfg(target_os = "windows")]
        let attributes = {
            use winit::platform::windows::WindowAttributesExtWindows;
            attributes.with_undecorated_shadow(true)
        };
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
            // Mailbox where the surface has it: frames still wait for the
            // display's refresh, so nothing tears, but a newer one replaces
            // one waiting. On Windows' Vulkan its swapchain is also made in
            // 40 ms, where FIFO's takes 170.
            present_mode: if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::AutoVsync
            },
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
            title: doc.title(),
            doc,
            access,
            pointer: (0.0, 0.0),
            canvas: None,
            canvas_by_gpu: false,
            cpu: render_cpu::Renderer::new(),
            pressed: None,
            choosing: false,
            blink: app::Blink::new(self.blinking),
            focused: false,
        };
        win.focused = win.window.has_focus();
        win.blink.moved(self.started.elapsed());
        let logical = win
            .window
            .inner_size()
            .to_logical::<f64>(win.window.scale_factor());
        win.doc.update(Event::Theme {
            dark: dark(&win.window),
        });
        win.doc.update(Event::Resize {
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
        let page = win.doc.view();
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
        let desk = if win.doc.dark() {
            self.res.dark.desk
        } else {
            self.res.light.desk
        };
        let drawn = if win.config.usage.contains(wgpu::TextureUsages::COPY_DST) {
            // Drawn into the window's canvas where the view changed, and
            // the canvas copied whole to the frame.
            let format = win.config.format;
            let canvas = match win.canvas.take() {
                Some(c) if c.size() == (w, h) && c.format() == format => c,
                _ => render_gpu::Canvas::new(gpu, w, h, format),
            };
            let canvas = win.canvas.insert(canvas);
            if win.canvas_by_gpu != drawn_by_gpu {
                // Another renderer draws all of it again.
                canvas.forget();
                win.canvas_by_gpu = drawn_by_gpu;
            }
            let rects = canvas.damage(&page, ppi);
            let drawn = match renderer {
                Some(renderer) => canvas.draw(gpu, renderer, page, ppi, desk, &rects),
                None => {
                    for rect in &rects {
                        let level = render_cpu::Level::Fastest;
                        let image = win.cpu.render_rect(&page, ppi, level, *rect);
                        canvas.write(gpu, *rect, &image.pixels);
                    }
                    canvas.shows(page, ppi);
                    Ok(())
                }
            };
            canvas.copy_to(gpu, &frame.texture);
            drawn
        } else {
            // A surface that takes no copies is drawn whole, by the GPU.
            win.canvas = None;
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            match renderer {
                Some(renderer) => renderer.render(gpu, &page, ppi, &view, w, h, desk),
                None => Err("no renderer for the surface".to_string()),
            }
        };
        if let Err(e) = drawn {
            eprintln!("xworks: drawing the window: {e}");
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

    /// Feeds `event` to window `id`'s document and carries out what it
    /// asks.
    fn feed(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: Event) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        // Typing, or moving the caret, shows it steadily for a while.
        if !matches!(
            event,
            Event::Move { .. }
                | Event::Scroll(_)
                | Event::Resize { .. }
                | Event::Theme { .. }
                | Event::Maximized(_)
        ) {
            win.blink.moved(self.started.elapsed());
            win.doc.show_caret(true);
        }
        let mut effects = win.doc.update(event);
        let mut opening = Vec::new();
        let mut closing = false;
        let mut quitting = false;
        while let Some(effect) = effects.pop() {
            match effect {
                Effect::Copy(text) => self.clipboard.copy(text),
                Effect::Paste => {
                    if let Some(text) = self.clipboard.paste() {
                        effects.extend(win.doc.update(Event::Pasted(text)));
                    }
                }
                Effect::Open(path) => opening.push((path, None)),
                Effect::OpenVersion(path, seq) => opening.push((Some(path), Some(seq))),
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
                Effect::Choose(choice) => {
                    if !win.choosing {
                        win.choosing = true;
                        let folder = match choice {
                            Choice::MoveTo => win
                                .doc
                                .editor()
                                .path()
                                .parent()
                                .map_or_else(|| self.res.documents.clone(), Path::to_path_buf),
                            _ => self.res.documents.clone(),
                        };
                        let name = win.doc.editor().name();
                        let markdown = win.doc.editor().is_markdown();
                        choose(
                            &win.window,
                            choice,
                            self.proxy.clone(),
                            &folder,
                            &name,
                            markdown,
                        );
                    }
                }
                Effect::Window(action) => match action {
                    app::WindowAction::Drag => {
                        let _ = win.window.drag_window();
                    }
                    app::WindowAction::Resize(edge) => {
                        let _ = win.window.drag_resize_window(direction(edge));
                    }
                    app::WindowAction::Minimize => win.window.set_minimized(true),
                    app::WindowAction::ToggleMaximize => {
                        let maximized = !win.window.is_maximized();
                        win.window.set_maximized(maximized);
                        effects.extend(win.doc.update(Event::Maximized(maximized)));
                    }
                    app::WindowAction::Close => closing = true,
                },
                Effect::Pointer(shape) => win.window.set_cursor(match shape {
                    app::PointerShape::Arrow => CursorIcon::Default,
                    app::PointerShape::Text => CursorIcon::Text,
                    app::PointerShape::Resize(edge) => CursorIcon::from(direction(edge)),
                }),
                Effect::Quit => quitting = true,
            }
        }
        // The input method's window goes below the caret, screen readers
        // see the text as it is, and the system the document's name.
        let (x, y, w, h) = win.doc.ime_area();
        win.window.set_ime_cursor_area(
            LogicalPosition::new(x, y),
            LogicalSize::new(w.max(1.0), h.max(1.0)),
        );
        let (doc, scale) = (&win.doc, win.window.scale_factor());
        win.access.update_if_active(|| tree(doc, scale));
        let title = win.doc.title();
        if title != win.title {
            win.window.set_title(&title);
            win.title = title;
        }
        win.window.request_redraw();
        // A setting changed, such as the ruler's unit, shows in every
        // window.
        let changes = self.res.settings.changes();
        if changes != self.settings_seen {
            self.settings_seen = changes;
            for other in self.windows.values() {
                other.window.request_redraw();
            }
        }
        if closing {
            self.close(event_loop, id);
        }
        if quitting {
            let ids: Vec<WindowId> = self.windows.keys().copied().collect();
            for id in ids {
                self.close(event_loop, id);
            }
            return;
        }
        for (path, version) in opening {
            if let Err(e) = self.open(event_loop, path, version) {
                eprintln!("xworks: {e:#}");
            }
        }
    }

    /// Closes window `id`: its document takes its name, if it isn't named
    /// by hand, and the app ends with its last window.
    fn close(&mut self, event_loop: &ActiveEventLoop, id: WindowId) {
        if let Some(mut win) = self.windows.remove(&id) {
            win.doc.close();
        }
        if self.windows.is_empty() {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(start) = self.start.take()
            && let Err(e) = self.open(event_loop, start, None)
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
            UserEvent::Blinking(blinking) => {
                self.blinking = blinking;
                for win in self.windows.values_mut() {
                    win.blink.set(blinking);
                }
                return;
            }
            UserEvent::Chosen(id, choice, path) => {
                if let Some(win) = self.windows.get_mut(&id) {
                    win.choosing = false;
                }
                self.feed(event_loop, id, Event::Chosen(choice, path));
                return;
            }
        };
        let Some(win) = self.windows.get_mut(&event.window_id) else {
            return;
        };
        if let accesskit_winit::WindowEvent::InitialTreeRequested = event.window_event {
            let (doc, scale) = (&win.doc, win.window.scale_factor());
            win.access.update_if_active(|| tree(doc, scale));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        win.access.process_event(&win.window, &event);
        let scale = win.window.scale_factor();
        match event {
            WindowEvent::CloseRequested => self.close(event_loop, id),
            WindowEvent::ThemeChanged(theme) => {
                if std::env::var_os("XWORKS_THEME").is_none() {
                    self.feed(
                        event_loop,
                        id,
                        Event::Theme {
                            dark: theme == Theme::Dark,
                        },
                    );
                }
            }
            WindowEvent::Resized(size) => {
                let (width, height) = (size.width.max(1), size.height.max(1));
                // A window says its size as it's shown, which its surface
                // has already: making its frames again costs a few
                // milliseconds at the start, and a software GPU's memory.
                if let Some(gpu) = &self.gpu
                    && (win.config.width, win.config.height) != (width, height)
                {
                    win.config.width = width;
                    win.config.height = height;
                    win.surface.configure(&gpu.device, &win.config);
                }
                let logical = size.to_logical::<f64>(scale);
                let maximized = win.window.is_maximized();
                self.feed(event_loop, id, Event::Maximized(maximized));
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
            // Alt pressed and let go with nothing between goes to the menus
            // on Windows and Linux.
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if matches!(event.logical_key, Key::Named(NamedKey::Alt)) => match event.state {
                ElementState::Pressed => self.alt_alone = !event.repeat,
                ElementState::Released => {
                    if std::mem::take(&mut self.alt_alone) && !cfg!(target_os = "macos") {
                        let event = Event::Key {
                            key: chrome::Key::Alt,
                            modifiers: chrome::Modifiers::default(),
                        };
                        self.feed(event_loop, id, event);
                    }
                }
            },
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed => {
                self.alt_alone = false;
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
                // Alt and a letter, without Ctrl (which with Alt is AltGr on
                // Windows), is a menu's access key on Windows and Linux; on a
                // Mac, Option types letters.
                let access = modifiers.alt && !m.control_key() && !cfg!(target_os = "macos");
                let typed = typed.or_else(|| {
                    if modifiers.primary || access {
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
                self.alt_alone = false;
                let (x, y) = win.pointer;
                let event = match state {
                    ElementState::Pressed => {
                        let now = Instant::now();
                        let count = match win.pressed {
                            Some((at, (px, py), n))
                                if now.duration_since(at) <= MULTI_CLICK_TIME
                                    && (x - px).abs() <= MULTI_CLICK_DISTANCE
                                    && (y - py).abs() <= MULTI_CLICK_DISTANCE =>
                            {
                                n + 1
                            }
                            _ => 1,
                        };
                        win.pressed = Some((now, (x, y), count));
                        Event::Press {
                            x,
                            y,
                            shift: self.modifiers.shift_key(),
                            count,
                        }
                    }
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
            // The caret blinks only in the window with the keyboard, and
            // shows steadily in the others; it comes back with the
            // keyboard as it does after typing.
            WindowEvent::Focused(focused) => {
                win.focused = focused;
                win.blink.moved(self.started.elapsed());
            }
            _ => {}
        }
    }

    /// Before the loop waits: each window's caret shows or goes as its
    /// blinking has it, drawn again where it changed, and the loop wakes
    /// for the next blink, or waits for events if none is coming.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = self.started.elapsed();
        let mut next: Option<Duration> = None;
        for win in self.windows.values_mut() {
            let shown = !win.focused || win.blink.shown(now);
            if shown != win.doc.caret_shown() {
                win.doc.show_caret(shown);
                win.window.request_redraw();
            }
            if win.focused
                && let Some(at) = win.blink.next(now)
            {
                next = Some(next.map_or(at, |n| n.min(at)));
            }
        }
        event_loop.set_control_flow(match next {
            Some(at) => ControlFlow::WaitUntil(self.started + at),
            None => ControlFlow::Wait,
        });
    }
}
