//! A window: one document's part, and the chrome xWorks draws around it
//! (ARCHITECTURE.md section 8, reviews frhewnk0 and 3ce247c2): its own
//! title bar with the menus, the document's name, Draft and Pages, the
//! inspector's button and the window's own buttons; the ruler; the
//! inspector; the status bar; and over them the menus, the inspector's
//! lists and the command palette. Light or dark, as the system is.
//!
//! Shells feed it [`Event`]s and carry out the [`Effect`]s it returns;
//! [`Window::view`] draws it. The document's part, the [`Editor`], gets
//! what falls in its area through the canvas contract (`part`), and the
//! commands the window doesn't handle itself.

mod menus;

use std::path::PathBuf;
use std::sync::Arc;

use chrome::{Arg, CommandState, Control, ControlKind, Key, MenuBar, MenuItem, Modifiers, Segment};
use part::{Input, Part, Request, Surface};
use scene::{Page, Point};
use text::units::PIXEL;
use toolkit::{
    ControlRef, Field, FieldKey, InspectorHit, InspectorView, Kit, PaletteHit, PaletteView, Popup,
    PopupRow, Rect, RulerHit, RulerView, Theme, TitleBar, TitleHit, TitleModel,
};

use crate::editor::{Edge, PointerShape, View, WindowAction};
use crate::{Choice, Editor, EditorError, Effect, Event, Resources};

use crate::recorder::{self, Checkpoint, Recorder};
use crate::session::{Chrome, Start};
use crate::settings;
use menus::MenuState;

/// The command palette, open: what's typed, what it finds, and which is
/// lit.
struct Palette {
    field: Field,
    found: Vec<chrome::Found>,
    selected: usize,
}

/// A drag on the ruler, and where it's got to, from the paragraph's
/// place's start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    /// A tab stop, from where it was; off the ruler, it goes.
    Stop { from: i64, to: i64, off: bool },
    /// The first line's indent.
    First(i64),
    /// The start indent.
    Start(i64),
    /// The end indent.
    End(i64),
    /// The pointer pressed in the document, which takes the drag.
    Part,
}

/// What the pointer is over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Hover {
    #[default]
    Nothing,
    Title(TitleHit),
    Inspector(InspectorHit),
    Palette(usize),
}

/// One document in one window, with its chrome.
pub struct Window {
    res: Arc<Resources>,
    editor: Editor,
    theme: Theme,
    dark: bool,
    width: i64,
    height: i64,
    maximized: bool,
    inspector: bool,
    tab: usize,
    ruler: bool,
    menu: Option<MenuState>,
    /// Whether the menus' access keys are underlined.
    access: bool,
    hover: Hover,
    palette: Option<Palette>,
    /// An inspector's choice open as a list, and which choice is lit.
    choosing: Option<(ControlRef, Option<usize>)>,
    /// An inspector's length being typed.
    editing: Option<(ControlRef, Field)>,
    /// The document's name being typed in the title bar.
    renaming: Option<Field>,
    drag: Option<Drag>,
    pointer: (i64, i64),
    shape: PointerShape,
    /// What records what's done in the window, if anything does.
    recorder: Option<Recorder>,
    /// Whether carets show: a shell blinking them hides them now and then
    /// ([`crate::Blink`]); headless, they always show.
    caret: bool,
}

/// Layout units from the logical pixels events carry.
fn units(v: f64) -> i64 {
    (v * PIXEL as f64).round() as i64
}

/// Cut, Copy and Paste in a small field, by their shortcuts.
fn clipboard(field: &mut Field, key: Key, modifiers: Modifiers) -> Option<Vec<Effect>> {
    if !modifiers.primary || modifiers.alt || modifiers.shift {
        return None;
    }
    match key {
        Key::Char('c') => Some(vec![Effect::Copy(field.selected().to_string())]),
        Key::Char('x') => {
            let cut = field.selected().to_string();
            field.insert("");
            Some(vec![Effect::Copy(cut)])
        }
        Key::Char('v') => Some(vec![Effect::Paste]),
        _ => None,
    }
}

impl Window {
    /// A window on a new, empty document, made at once in the documents
    /// folder.
    pub fn create(res: Arc<Resources>) -> Result<Window, EditorError> {
        let editor = Editor::create(res.clone())?;
        Ok(Window::with(res, editor))
    }

    /// A window on the document at `path`.
    pub fn open(res: Arc<Resources>, path: &std::path::Path) -> Result<Window, EditorError> {
        let editor = Editor::open(res.clone(), path)?;
        Ok(Window::with(res, editor))
    }

    /// A window on the named version of the document at `path` that
    /// follows its log entry `seq`, read-only.
    pub fn open_version(
        res: Arc<Resources>,
        path: &std::path::Path,
        seq: i64,
    ) -> Result<Window, EditorError> {
        let editor = Editor::open_version(res.clone(), path, seq)?;
        Ok(Window::with(res, editor))
    }

    /// A window taking up a recorded session where it starts, `size`
    /// logical pixels: its document, made a new one in the documents
    /// folder, and the window as it was ([`Editor::resume`]).
    pub fn resume(
        res: Arc<Resources>,
        start: &Start,
        size: (f64, f64),
    ) -> Result<Window, EditorError> {
        let editor = Editor::resume(res.clone(), start, size)?;
        let mut window = Window::with(res, editor);
        if let Some(chrome) = start.chrome {
            window.inspector = chrome.inspector.is_some();
            window.tab = chrome.inspector.unwrap_or(0);
            window.ruler = chrome.ruler;
            window.dark = chrome.dark;
            window.theme = window.res.theme(chrome.dark);
            if let Some(units) = chrome.units {
                window.res.settings.set_units(units);
            }
        }
        Ok(window)
    }

    /// Records what's done in this window from now on, with `recorder`.
    pub fn record(&mut self, recorder: Recorder) {
        recorder.begin(self);
        self.recorder = Some(recorder);
    }

    /// The window's state, if it's at rest, so a recorded session can
    /// start there: no menu, list, panel, field or drag is open, and the
    /// editor is at rest too.
    pub(crate) fn checkpoint(&self) -> Option<Checkpoint> {
        if self.menu.is_some()
            || self.palette.is_some()
            || self.choosing.is_some()
            || self.editing.is_some()
            || self.renaming.is_some()
            || self.drag.is_some()
        {
            return None;
        }
        let mut checkpoint = self.editor.checkpoint()?;
        let px = |v: i64| v as f64 / PIXEL as f64;
        checkpoint.size = (px(self.width), px(self.height));
        checkpoint.chrome = Some(Chrome {
            draft: self.editor.view_mode() == View::Draft,
            inspector: self.inspector.then_some(self.tab),
            ruler: self.ruler,
            dark: self.dark,
            units: Some(self.res.settings.units()),
        });
        Some(checkpoint)
    }

    /// Writes what the recorder kept of this window to the sessions
    /// folder, and says where.
    fn save_session(&mut self) {
        let Some(recorder) = &self.recorder else {
            self.editor.say("Nothing is recorded in this window");
            return;
        };
        let folder = recorder::folder(&self.res.documents);
        let message = match recorder.write_to(&folder, "Saved by Save Recent Session.") {
            Ok(path) => format!("Saved the last few minutes to {}", path.display()),
            Err(e) => format!("Couldn't save the recent session: {e}"),
        };
        self.editor.say(&message);
    }

    /// A window holding `editor`.
    pub fn with(res: Arc<Resources>, editor: Editor) -> Window {
        res.recent.add(editor.path());
        let theme = res.theme(false);
        Window {
            res,
            editor,
            theme,
            dark: false,
            width: 1000 * PIXEL,
            height: 800 * PIXEL,
            maximized: false,
            inspector: false,
            tab: 0,
            ruler: true,
            menu: None,
            access: false,
            hover: Hover::Nothing,
            palette: None,
            choosing: None,
            editing: None,
            renaming: None,
            drag: None,
            pointer: (0, 0),
            shape: PointerShape::Arrow,
            recorder: None,
            caret: true,
        }
    }

    /// The document's part.
    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// The window's title, as the system shows it.
    pub fn title(&self) -> String {
        format!("{} — xWorks", self.editor.name())
    }

    /// Whether the window is dark.
    pub fn dark(&self) -> bool {
        self.dark
    }

    /// Whether the inspector is open.
    pub fn inspector_open(&self) -> bool {
        self.inspector
    }

    /// Shows carets, or hides them, as a blinking caret is gone now and
    /// then: the document's and a field's being typed in. It isn't an
    /// event: nothing records it, and only the carets draw differently.
    pub fn show_caret(&mut self, shown: bool) {
        self.caret = shown;
    }

    /// Whether carets show.
    pub fn caret_shown(&self) -> bool {
        self.caret
    }

    /// The window is closing: the document takes the name its text gives
    /// it, if it isn't named by hand.
    pub fn close(&mut self) {
        let from = self.editor.path().to_path_buf();
        self.editor.settle_name();
        self.moved(&from);
    }

    fn kit(&self) -> Kit<'_> {
        Kit {
            fonts: self.res.fonts(),
            theme: &self.theme,
            icons: &self.res.icons,
            labels: &self.res.labels,
            caret: self.caret,
        }
    }

    // ---- where things are

    fn title_height(&self) -> i64 {
        self.res.sizes.title_bar.units()
    }

    fn status_height(&self) -> i64 {
        self.res.sizes.status_bar.units()
    }

    fn ruler_shown(&self) -> bool {
        self.ruler
    }

    /// The document's area.
    fn area(&self) -> Rect {
        let s = &self.res.sizes;
        let top = self.title_height()
            + if self.ruler_shown() {
                s.ruler.units()
            } else {
                0
            };
        let right = if self.inspector {
            self.width - s.inspector.units()
        } else {
            self.width
        };
        let bottom = self.height - self.status_height();
        Rect::new(0, top, right.max(PIXEL), (bottom - top).max(PIXEL))
    }

    fn surface(&self) -> Surface {
        let area = self.area();
        Surface {
            width: area.width,
            height: area.height,
        }
    }

    fn menu_bar(&self) -> MenuBar {
        self.res.commands.menu_bar(
            self.res.mac,
            |id| self.state(id),
            |id| match id {
                // Read only while a menu is open: the list's file isn't
                // touched until File, Open Recent can show it.
                "open-recent" if self.menu.is_some_and(|m| m.open) => self
                    .res
                    .recent
                    .paths()
                    .into_iter()
                    .filter(|p| p != self.editor.path())
                    .map(|p| MenuItem::Command {
                        id: "open-recent".to_string(),
                        title: p
                            .file_stem()
                            .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
                        args: vec![Arg::Text(p.to_string_lossy().into_owned())],
                        access: None,
                        shortcut: None,
                        enabled: true,
                        checked: false,
                    })
                    .collect(),
                _ => Vec::new(),
            },
        )
    }

    /// The contextual buttons in the title bar, right to left: the
    /// inspector's, then Draft and Pages.
    fn buttons(&self) -> Vec<Control> {
        let segment = |command: &str| Segment {
            command: command.to_string(),
            label: self
                .res
                .commands
                .get(command)
                .map_or_else(String::new, |c| c.title.clone()),
            icon: None,
        };
        vec![
            Control {
                command: "toggle-inspector".to_string(),
                args: Vec::new(),
                enabled: true,
                kind: ControlKind::Toggle {
                    icon: "panel-right".to_string(),
                    label: "Inspector".to_string(),
                    on: self.inspector,
                },
            },
            Control {
                command: String::new(),
                args: Vec::new(),
                enabled: true,
                kind: ControlKind::Segments {
                    segments: vec![segment("view-draft"), segment("view-pages")],
                    selected: Some(match self.editor.view_mode() {
                        View::Draft => 0,
                        View::Pages => 1,
                    }),
                },
            },
        ]
    }

    fn title_bar(&self, menus: &MenuBar, buttons: &[Control], name: &str) -> TitleBar {
        TitleBar::new(
            &self.kit(),
            &self.title_model(menus, buttons, name),
            self.width,
        )
    }

    fn title_model<'a>(
        &'a self,
        menus: &'a MenuBar,
        buttons: &'a [Control],
        name: &'a str,
    ) -> TitleModel<'a> {
        TitleModel {
            menus,
            open: self.menu.as_ref().map(|m| m.index),
            access: self.access && !self.res.mac,
            name,
            renaming: self.renaming.as_ref(),
            buttons,
            maximized: self.maximized,
        }
    }

    fn ruler_view(&self) -> Option<RulerView> {
        if !self.ruler_shown() {
            return None;
        }
        let mut model = Part::ruler(&self.editor)?;
        // A marker being dragged shows where it's got to.
        if let Some(drag) = self.drag
            && let Some(indents) = &mut model.indents
        {
            match drag {
                Drag::Stop { from, to, off } => {
                    if let Some(stop) = model.stops.iter_mut().find(|s| s.position == from) {
                        stop.position = to;
                    }
                    if off {
                        model.stops.retain(|s| s.position != to);
                    }
                }
                Drag::First(at) => indents.first = at - indents.start,
                Drag::Start(at) => indents.start = at,
                Drag::End(at) => indents.end = model.extent - at,
                Drag::Part => {}
            }
        }
        Some(RulerView::new(
            &self.kit(),
            &model,
            self.title_height(),
            self.area().width,
        ))
    }

    fn inspector_view(&self, model: &chrome::Inspector) -> Option<InspectorView> {
        self.inspector.then(|| {
            InspectorView::new(
                &self.kit(),
                model,
                self.tab,
                self.width,
                self.title_height(),
                self.height - self.status_height(),
            )
        })
    }

    fn palette_view(&self) -> Option<PaletteView> {
        let palette = self.palette.as_ref()?;
        Some(PaletteView::new(
            &self.kit(),
            palette.found.len(),
            self.width,
            self.title_height(),
        ))
    }

    /// The inspector's fields, while it's open: the editor describes them
    /// only then.
    fn inspector_model(&self) -> chrome::Inspector {
        if self.inspector {
            Part::inspector(&self.editor)
        } else {
            chrome::Inspector::default()
        }
    }

    /// The control `at` of the inspector's tab shown.
    fn control(model: &chrome::Inspector, tab: usize, at: ControlRef) -> Option<&Control> {
        model
            .tabs
            .get(tab)?
            .sections
            .get(at.section)?
            .rows
            .get(at.row)?
            .controls
            .get(at.control)
    }

    /// The list of an inspector's choice, open under it.
    fn choice_popup(&self, model: &chrome::Inspector) -> Option<(Popup, Vec<PopupRow>)> {
        let (at, _) = self.choosing?;
        let control = Window::control(model, self.tab, at)?;
        let ControlKind::Choice {
            choices, selected, ..
        } = &control.kind
        else {
            return None;
        };
        let rows: Vec<PopupRow> = choices
            .iter()
            .enumerate()
            .map(|(i, c)| PopupRow::choice(&c.label, *selected == Some(i)))
            .collect();
        let field = self.inspector_view(model)?.control(at)?;
        let popup = Popup::new(
            &self.kit(),
            &rows,
            (field.x, field.bottom() + 2 * PIXEL),
            field.width,
            (self.width, self.height),
        );
        Some((popup, rows))
    }

    /// The editor's panel, open, as a list under the title bar: its
    /// title, a line, and its rows.
    fn panel_popup(&self) -> Option<(Popup, Vec<PopupRow>)> {
        let (title, names, _) = self.editor.panel()?;
        let mut rows = vec![
            PopupRow {
                enabled: false,
                ..PopupRow::choice(title, false)
            },
            PopupRow::choice("", false),
        ];
        rows.extend(names.iter().map(|n| PopupRow::choice(n, false)));
        let width = self.res.sizes.palette.units().min(self.width);
        let kit = self.kit();
        let popup = Popup::new(
            &kit,
            &rows,
            (
                (self.width - width) / 2,
                self.title_height() + self.res.sizes.palette_top.units(),
            ),
            width,
            (self.width, self.height),
        );
        Some((popup, rows))
    }

    // ---- commands

    /// Whether command `id` can run now, and whether it's on.
    pub fn state(&self, id: &str) -> CommandState {
        let on = |checked| CommandState {
            enabled: true,
            checked,
        };
        match id {
            "toggle-inspector" => on(self.inspector),
            "toggle-ruler" => on(self.ruler),
            "command-palette" | "about" | "close" => on(false),
            "rename" => CommandState {
                enabled: self.editor.writable(),
                checked: false,
            },
            "open-recent" => CommandState {
                enabled: !self.res.recent.paths().is_empty(),
                checked: false,
            },
            _ if let Some(unit) = settings::unit_of(id) => on(self.res.settings.units() == unit),
            _ => Part::state(&self.editor, id),
        }
    }

    /// Runs command `id` with `args`: the window's own, or the
    /// document's.
    fn run(&mut self, id: &str, args: Vec<Arg>) -> Vec<Effect> {
        self.menu = None;
        self.access = false;
        self.palette = None;
        self.choosing = None;
        match id {
            "toggle-inspector" => self.inspector = !self.inspector,
            "toggle-ruler" => self.ruler = !self.ruler,
            "command-palette" => self.open_palette(),
            "about" => self.editor_message("xWorks, M2: foundations for documents"),
            "save-session" => self.save_session(),
            "close" => return vec![Effect::Window(WindowAction::Close)],
            "rename" => self.renaming = Some(Field::new(&self.editor.name())),
            "open-recent" => {
                if let Some(Arg::Text(path)) = args.first() {
                    return vec![Effect::Open(Some(PathBuf::from(path)))];
                }
            }
            _ if let Some(unit) = settings::unit_of(id) => self.res.settings.set_units(unit),
            _ => {
                let takes = self.res.commands.get(id).is_some_and(|c| c.arg.is_some());
                let input = if args.is_empty() && !takes {
                    Input::Command(id.to_string())
                } else {
                    Input::CommandWith(id.to_string(), args)
                };
                return self.give_part(input);
            }
        }
        self.resize_part();
        Vec::new()
    }

    fn editor_message(&mut self, message: &str) {
        self.editor.say(message);
    }

    /// Gives the part `input`, and turns what it asks into effects; the
    /// document takes the name its text gives it once the caret leaves
    /// the paragraph it was in.
    fn give_part(&mut self, input: Input) -> Vec<Effect> {
        let before = self.editor.selection().1.paragraph;
        let requests = self.editor.input(input);
        if self.editor.selection().1.paragraph != before && !self.editor.named_by_hand() {
            let from = self.editor.path().to_path_buf();
            self.editor.settle_name();
            self.moved(&from);
        }
        requests
            .into_iter()
            .map(|request| match request {
                Request::Copy(text) => Effect::Copy(text),
                Request::Paste => Effect::Paste,
                Request::Choose(choice) => Effect::Choose(choice),
                Request::Open(path) => Effect::Open(path),
                Request::OpenVersion(path, seq) => Effect::OpenVersion(path, seq),
                Request::OpenExternally(path) => Effect::OpenExternally(path),
                Request::Reveal(path) => Effect::Reveal(path),
                Request::Quit => Effect::Quit,
            })
            .collect()
    }

    /// Tells the recent documents where the document went, if it moved
    /// from `from`.
    fn moved(&self, from: &std::path::Path) {
        if from != self.editor.path() {
            self.res.recent.moved(from, self.editor.path());
        }
    }

    fn resize_part(&mut self) {
        let surface = self.surface();
        self.editor.resize(surface);
    }

    // ---- the palette

    fn open_palette(&mut self) {
        let mut palette = Palette {
            field: Field::new(""),
            found: Vec::new(),
            selected: 0,
        };
        palette.found = self.find("");
        self.palette = Some(palette);
    }

    fn find(&self, query: &str) -> Vec<chrome::Found> {
        self.res
            .commands
            .find(query, self.res.mac, |c| self.state(&c.id).enabled)
    }

    fn palette_key(&mut self, key: Key, modifiers: Modifiers) -> Vec<Effect> {
        let layout = {
            let palette = self.palette.as_ref().expect("the palette is open");
            let ty = self.kit().ty(self.res.sizes.palette_text, 400);
            self.kit().layout(palette.field.text(), ty)
        };
        let mac = self.res.mac;
        let Some(palette) = &mut self.palette else {
            return Vec::new();
        };
        if let Some(effects) = clipboard(&mut palette.field, key, modifiers) {
            self.refind();
            return effects;
        }
        match key {
            Key::Down => {
                palette.selected = (palette.selected + 1).min(palette.found.len().saturating_sub(1))
            }
            Key::Up => palette.selected = palette.selected.saturating_sub(1),
            Key::Enter => {
                let chosen = palette.found.get(palette.selected).map(|f| f.id.clone());
                self.palette = None;
                if let Some(id) = chosen {
                    return self.run(&id, Vec::new());
                }
            }
            Key::Escape => self.palette = None,
            _ => {
                if palette.field.key(&layout, key, modifiers, mac) == FieldKey::Changed {
                    self.refind();
                }
            }
        }
        Vec::new()
    }

    fn refind(&mut self) {
        let query = match &self.palette {
            Some(p) => p.field.text().to_string(),
            None => return,
        };
        let found = self.find(&query);
        if let Some(palette) = &mut self.palette {
            palette.found = found;
            palette.selected = 0;
        }
    }

    /// The first command shown, so the one lit is in view.
    fn palette_first(palette: &Palette) -> usize {
        palette.selected.saturating_sub(toolkit::SHOWN - 1)
    }

    // ---- fields: the name, and lengths in the inspector

    fn field_key(&mut self, key: Key, modifiers: Modifiers) -> Option<Vec<Effect>> {
        let mac = self.res.mac;
        if let Some(field) = self
            .renaming
            .as_mut()
            .or(self.editing.as_mut().map(|(_, f)| f))
            && let Some(effects) = clipboard(field, key, modifiers)
        {
            return Some(effects);
        }
        if self.renaming.is_some() {
            let ty = self.kit().ty(self.res.sizes.text, 500);
            let layout = self.kit().layout(self.renaming.as_ref()?.text(), ty);
            let field = self.renaming.as_mut()?;
            match field.key(&layout, key, modifiers, mac) {
                FieldKey::Enter | FieldKey::Tab { .. } => self.finish_renaming(),
                FieldKey::Escape => self.renaming = None,
                _ => {}
            }
            return Some(Vec::new());
        }
        if self.editing.is_some() {
            let ty = self.kit().ty(self.res.sizes.value, 400);
            let layout = self.kit().layout(self.editing.as_ref()?.1.text(), ty);
            let (_, field) = self.editing.as_mut()?;
            return Some(match field.key(&layout, key, modifiers, mac) {
                FieldKey::Enter | FieldKey::Tab { .. } => self.finish_editing(),
                FieldKey::Escape => {
                    self.editing = None;
                    Vec::new()
                }
                _ => Vec::new(),
            });
        }
        None
    }

    fn finish_renaming(&mut self) {
        let Some(field) = self.renaming.take() else {
            return;
        };
        let name = field.text().trim().to_string();
        if name.is_empty() || name == self.editor.name() && self.editor.named_by_hand() {
            return;
        }
        let from = self.editor.path().to_path_buf();
        if let Err(e) = self.editor.rename(&name) {
            self.editor_message(&format!("Couldn't rename it: {e}"));
        }
        self.moved(&from);
    }

    /// Runs the length typed into the inspector, if it reads as one.
    fn finish_editing(&mut self) -> Vec<Effect> {
        let Some((at, field)) = self.editing.take() else {
            return Vec::new();
        };
        let model = Part::inspector(&self.editor);
        let Some(control) = Window::control(&model, self.tab, at) else {
            return Vec::new();
        };
        let ControlKind::Length { value, .. } = &control.kind else {
            return Vec::new();
        };
        // A length left as it's shown stays as it is, not as it reads back:
        // a stop at 3.5 cm shows in picas as 8p3.21, a hair short of it.
        if value.is_some_and(|v| v.to_string() == field.text().trim()) {
            return Vec::new();
        }
        let unit = value.map_or(chrome::Unit::Point, |v| v.unit);
        match chrome::Length::parse(field.text(), unit) {
            Some(length) => {
                let args = control.with(Arg::Int(length.value));
                let command = control.command.clone();
                self.run(&command, args)
            }
            None => {
                self.editor_message(&format!("{:?} isn't a length", field.text()));
                Vec::new()
            }
        }
    }

    // ---- events

    /// Handles `event`, returning what the shell should do. A recorder,
    /// if the window has one, notes it.
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        let Some(recorder) = self.recorder.clone() else {
            return self.handle(event);
        };
        recorder.before(self, &event);
        let effects = self.handle(event);
        recorder.after(self);
        effects
    }

    fn handle(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Key { key, modifiers } => self.key(key, modifiers),
            Event::Text(text) => {
                if let Some(field) = &mut self.renaming {
                    field.insert(&text);
                } else if let Some((_, field)) = &mut self.editing {
                    field.insert(&text);
                } else if let Some(palette) = &mut self.palette {
                    palette.field.insert(&text);
                    self.refind();
                } else if self.menu.is_some() {
                    // Typing in a menu picks by access key.
                    if let Some(c) = text.chars().next() {
                        return self.menu_key(
                            Key::Char(c.to_lowercase().next().unwrap_or(c)),
                            Modifiers::default(),
                        );
                    }
                } else {
                    return self.give_part(Input::Text(text));
                }
                Vec::new()
            }
            Event::Preedit(text) => self.give_part(Input::Preedit(text)),
            Event::Press { x, y, shift, count } => self.press(units(x), units(y), shift, count),
            Event::Move { x, y } => self.pointer_moved(units(x), units(y)),
            Event::Release => self.release(),
            Event::Scroll(dy) => {
                let (x, y) = self.pointer;
                if self
                    .inspector_view(&chrome::Inspector::default())
                    .is_some_and(|v| v.rect().contains(x, y))
                {
                    return Vec::new();
                }
                self.give_part(Input::Scroll(units(dy)))
            }
            Event::Resize { width, height } => {
                self.width = units(width).max(PIXEL);
                self.height = units(height).max(PIXEL);
                self.resize_part();
                Vec::new()
            }
            Event::Command(id) => self.run(&id, Vec::new()),
            Event::CommandWith(id, args) => self.run(&id, args),
            Event::Pasted(text) => {
                if let Some(field) = &mut self.renaming {
                    field.insert(&text);
                } else if let Some((_, field)) = &mut self.editing {
                    field.insert(&text);
                } else if let Some(palette) = &mut self.palette {
                    palette.field.insert(&text);
                    self.refind();
                } else {
                    return self.give_part(Input::Pasted(text));
                }
                Vec::new()
            }
            Event::Chosen(choice, path) => {
                let from = self.editor.path().to_path_buf();
                let effects = self.give_part(Input::Chosen(choice, path));
                if choice == Choice::MoveTo {
                    self.moved(&from);
                }
                effects
            }
            Event::Theme { dark } => {
                self.dark = dark;
                self.theme = self.res.theme(dark);
                Vec::new()
            }
            Event::Maximized(maximized) => {
                self.maximized = maximized;
                Vec::new()
            }
        }
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) -> Vec<Effect> {
        if let Some(effects) = self.field_key(key, modifiers) {
            return effects;
        }
        if self.editor.panel().is_some() {
            return self.editor.panel_key(key);
        }
        if self.palette.is_some() {
            return self.palette_key(key, modifiers);
        }
        if self.choosing.is_some() {
            return self.choice_key(key);
        }
        if self.menu.is_some() {
            return self.menu_key(key, modifiers);
        }
        if !self.res.mac {
            // Alt alone, or F10, goes to the menus; Alt and a letter opens
            // the menu it's the access key of.
            if matches!(key, Key::Alt | Key::F10) && !modifiers.primary {
                self.focus_menus(0, false);
                return Vec::new();
            }
            if modifiers.alt
                && !modifiers.primary
                && let Key::Char(c) = key
            {
                let bar = self.menu_bar();
                if let Some(i) = bar.menus.iter().position(|m| m.access == Some(c)) {
                    self.focus_menus(i, true);
                    return Vec::new();
                }
            }
        }
        if let Some(command) = self.res.commands.for_key(key, modifiers, self.res.mac) {
            let id = command.id.clone();
            return self.run(&id, Vec::new());
        }
        self.give_part(Input::Key { key, modifiers })
    }

    fn choice_key(&mut self, key: Key) -> Vec<Effect> {
        let model = Part::inspector(&self.editor);
        let Some((at, lit)) = self.choosing else {
            return Vec::new();
        };
        let count = match Window::control(&model, self.tab, at).map(|c| &c.kind) {
            Some(ControlKind::Choice { choices, .. }) => choices.len(),
            _ => 0,
        };
        match key {
            Key::Down => {
                self.choosing = Some((
                    at,
                    Some(lit.map_or(0, |l| (l + 1).min(count.saturating_sub(1)))),
                ))
            }
            Key::Up => self.choosing = Some((at, Some(lit.map_or(0, |l| l.saturating_sub(1))))),
            Key::Enter => {
                if let Some(l) = lit {
                    return self.choose(&model, at, l);
                }
                self.choosing = None;
            }
            Key::Escape => self.choosing = None,
            _ => {}
        }
        Vec::new()
    }

    /// Runs choice `i` of the inspector's control `at`.
    fn choose(&mut self, model: &chrome::Inspector, at: ControlRef, i: usize) -> Vec<Effect> {
        self.choosing = None;
        let Some(control) = Window::control(model, self.tab, at) else {
            return Vec::new();
        };
        let ControlKind::Choice { choices, .. } = &control.kind else {
            return Vec::new();
        };
        match choices.get(i) {
            Some(choice) => {
                let args = control.with(choice.value.clone());
                let command = control.command.clone();
                self.run(&command, args)
            }
            None => Vec::new(),
        }
    }

    /// Which edge of the window (`x`, `y`) is on, where a drag resizes it.
    fn edge(&self, x: i64, y: i64) -> Option<Edge> {
        if self.maximized {
            return None;
        }
        let reach = self.res.sizes.resize_edge.units();
        let (w, n, e, s) = (
            x < reach,
            y < reach,
            x >= self.width - reach,
            y >= self.height - reach,
        );
        Some(match (n, s, w, e) {
            (true, _, true, _) => Edge::NorthWest,
            (true, _, _, true) => Edge::NorthEast,
            (_, true, true, _) => Edge::SouthWest,
            (_, true, _, true) => Edge::SouthEast,
            (true, ..) => Edge::North,
            (_, true, ..) => Edge::South,
            (_, _, true, _) => Edge::West,
            (_, _, _, true) => Edge::East,
            _ => return None,
        })
    }

    fn press(&mut self, x: i64, y: i64, shift: bool, count: u32) -> Vec<Effect> {
        self.pointer = (x, y);
        // What's typed is done when the pointer goes elsewhere.
        if self.renaming.is_some() {
            let menus = self.menu_bar();
            let buttons = self.buttons();
            let name = self.editor.name();
            let bar = self.title_bar(&menus, &buttons, &name);
            if bar.hit(x, y) != Some(TitleHit::Name) {
                self.finish_renaming();
            }
        }
        let mut effects = Vec::new();
        let model = self.inspector_model();
        if let Some((at, _)) = &self.editing {
            let at = *at;
            let inside = self
                .inspector_view(&model)
                .and_then(|v| v.control(at))
                .is_some_and(|r| r.contains(x, y));
            if !inside {
                effects.extend(self.finish_editing());
            }
        }
        // Lists over everything take the press first.
        if let Some((popup, _)) = self.panel_popup() {
            match popup.hit(x, y) {
                Some(Some(row)) if row >= 2 => effects.extend(self.editor.choose_row(row - 2)),
                Some(_) => {}
                None => self.editor.close_panel(),
            }
            return effects;
        }
        if self.choosing.is_some() {
            if let Some((popup, _)) = self.choice_popup(&model)
                && let Some(row) = popup.hit(x, y)
            {
                if let (Some(row), Some((at, _))) = (row, self.choosing) {
                    effects.extend(self.choose(&model, at, row));
                }
                return effects;
            }
            self.choosing = None;
            return effects;
        }
        if let Some(view) = self.palette_view() {
            match view.hit(x, y) {
                Some(PaletteHit::Row(i)) => {
                    let first = self.palette.as_ref().map_or(0, Window::palette_first);
                    let id = self
                        .palette
                        .as_ref()
                        .and_then(|p| p.found.get(first + i))
                        .map(|f| f.id.clone());
                    self.palette = None;
                    if let Some(id) = id {
                        effects.extend(self.run(&id, Vec::new()));
                    }
                }
                Some(_) => {}
                None => self.palette = None,
            }
            return effects;
        }
        if self.menu.is_some()
            && let Some(more) = self.menu_press(x, y)
        {
            effects.extend(more);
            return effects;
        }
        if let Some(edge) = self.edge(x, y) {
            effects.push(Effect::Window(WindowAction::Resize(edge)));
            return effects;
        }
        // The title bar.
        let menus = self.menu_bar();
        let buttons = self.buttons();
        let name = self.editor.name();
        let bar = self.title_bar(&menus, &buttons, &name);
        if let Some(hit) = bar.hit(x, y) {
            match hit {
                TitleHit::Menu(i) => self.open_menu(i, false),
                TitleHit::Name => {
                    if self.renaming.is_none() {
                        self.renaming = Some(Field::new(&name));
                    }
                }
                TitleHit::Button(i, k) => match buttons.get(i).map(|b| &b.kind) {
                    Some(ControlKind::Segments { segments, .. }) => {
                        if let Some(command) = segments.get(k).map(|s| s.command.clone()) {
                            effects.extend(self.run(&command, Vec::new()));
                        }
                    }
                    Some(_) => {
                        let command = buttons[i].command.clone();
                        effects.extend(self.run(&command, Vec::new()));
                    }
                    None => {}
                },
                TitleHit::Minimize => effects.push(Effect::Window(WindowAction::Minimize)),
                TitleHit::Maximize => effects.push(Effect::Window(WindowAction::ToggleMaximize)),
                TitleHit::Close => effects.push(Effect::Window(WindowAction::Close)),
                TitleHit::Drag if count >= 2 => {
                    effects.push(Effect::Window(WindowAction::ToggleMaximize));
                }
                TitleHit::Drag => effects.push(Effect::Window(WindowAction::Drag)),
            }
            return effects;
        }
        // The ruler.
        if let Some(ruler) = self.ruler_view()
            && let Some(hit) = ruler.hit(x, y)
        {
            let at = ruler.snapped(x);
            let model = Part::ruler(&self.editor);
            match hit {
                RulerHit::Stop(i) => {
                    if let Some(stop) = model.and_then(|m| m.stops.get(i).copied()) {
                        self.drag = Some(Drag::Stop {
                            from: stop.position,
                            to: stop.position,
                            off: false,
                        });
                    }
                }
                RulerHit::First => self.drag = Some(Drag::First(ruler.position(x))),
                RulerHit::Start => self.drag = Some(Drag::Start(ruler.position(x))),
                RulerHit::End => self.drag = Some(Drag::End(ruler.position(x))),
                RulerHit::Foot(_) => {
                    effects.extend(self.run("tab-stop-add", vec![Arg::Int(at.max(0))]));
                    self.drag = Some(Drag::Stop {
                        from: at.max(0),
                        to: at.max(0),
                        off: false,
                    });
                }
                RulerHit::Bar => {}
            }
            return effects;
        }
        // The inspector.
        if let Some(view) = self.inspector_view(&model)
            && let Some(hit) = view.hit(x, y)
        {
            match hit {
                InspectorHit::Tab(k) => {
                    self.tab = k;
                    self.editing = None;
                }
                InspectorHit::Control(at, k) => {
                    if let Some(control) = Window::control(&model, self.tab, at)
                        && control.enabled
                    {
                        match &control.kind {
                            ControlKind::Choice { selected, .. } => {
                                self.choosing = Some((at, *selected));
                            }
                            ControlKind::Length { value, .. } => {
                                let shown = value.map_or_else(String::new, |v| v.to_string());
                                self.editing = Some((at, Field::new(&shown)));
                            }
                            ControlKind::Segments { segments, .. } => {
                                if let Some(command) = segments.get(k).map(|s| s.command.clone()) {
                                    effects.extend(self.run(&command, Vec::new()));
                                }
                            }
                            ControlKind::Toggle { .. } | ControlKind::Button { .. } => {
                                let (command, args) =
                                    (control.command.clone(), control.args.clone());
                                effects.extend(self.run(&command, args));
                            }
                        }
                    }
                }
                InspectorHit::Panel => {}
            }
            return effects;
        }
        // The document.
        let area = self.area();
        if area.contains(x, y) {
            self.drag = Some(Drag::Part);
            effects.extend(self.give_part(Input::Press {
                x: x - area.x,
                y: y - area.y,
                shift,
                count,
            }));
        }
        effects
    }

    fn pointer_moved(&mut self, x: i64, y: i64) -> Vec<Effect> {
        self.pointer = (x, y);
        let mut effects = Vec::new();
        let area = self.area();
        match self.drag {
            Some(Drag::Part) => {
                return self.give_part(Input::Move {
                    x: x - area.x,
                    y: y - area.y,
                });
            }
            Some(drag) => {
                if let Some(ruler) = self.ruler_view() {
                    let at = ruler.snapped(x);
                    let r = ruler.rect();
                    let off = y < r.y - 24 * PIXEL || y > r.bottom() + 24 * PIXEL;
                    self.drag = Some(match drag {
                        Drag::Stop { from, .. } => Drag::Stop {
                            from,
                            to: at.max(0),
                            off,
                        },
                        Drag::First(_) => Drag::First(at),
                        Drag::Start(_) => Drag::Start(at),
                        Drag::End(_) => Drag::End(at),
                        Drag::Part => Drag::Part,
                    });
                }
                return effects;
            }
            None => {}
        }
        // Menus follow the pointer while one is open.
        if self.menu.is_some() {
            self.menu_hover(x, y);
        }
        let model = self.inspector_model();
        if let Some((popup, _)) = self.choice_popup(&model)
            && let Some((at, _)) = self.choosing
        {
            if let Some(row) = popup.hit(x, y) {
                self.choosing = Some((at, row));
            }
            return effects;
        }
        self.hover = Hover::Nothing;
        let menus = self.menu_bar();
        let buttons = self.buttons();
        let name = self.editor.name();
        let bar = self.title_bar(&menus, &buttons, &name);
        let mut shape = PointerShape::Arrow;
        if let Some(view) = self.palette_view() {
            if let Some(PaletteHit::Row(i)) = view.hit(x, y)
                && let Some(palette) = &mut self.palette
            {
                // The pointer lights a command as the arrows do.
                palette.selected = Window::palette_first(palette) + i;
                self.hover = Hover::Palette(i);
            }
        } else if let Some(edge) = self.edge(x, y) {
            shape = PointerShape::Resize(edge);
        } else if let Some(hit) = bar.hit(x, y) {
            self.hover = Hover::Title(hit);
        } else if let Some(hit) = self.inspector_view(&model).and_then(|v| v.hit(x, y)) {
            self.hover = Hover::Inspector(hit);
        } else if area.contains(x, y) && self.menu.is_none() {
            if self.editor.pointer(x - area.x, y - area.y) == part::Pointer::Text {
                shape = PointerShape::Text;
            }
            effects.extend(self.give_part(Input::Move {
                x: x - area.x,
                y: y - area.y,
            }));
        }
        if shape != self.shape {
            self.shape = shape;
            effects.push(Effect::Pointer(shape));
        }
        effects
    }

    fn release(&mut self) -> Vec<Effect> {
        let Some(drag) = self.drag.take() else {
            return Vec::new();
        };
        match drag {
            Drag::Part => self.give_part(Input::Release),
            Drag::Stop { from, to, off } => {
                if off {
                    self.run("tab-stop-remove", vec![Arg::Int(from)])
                } else if to != from {
                    self.run("tab-stop-move", vec![Arg::Int(from), Arg::Int(to)])
                } else {
                    Vec::new()
                }
            }
            Drag::First(at) => {
                let start = Part::ruler(&self.editor)
                    .and_then(|r| r.indents)
                    .map_or(0, |i| i.start);
                self.run("indent-first", vec![Arg::Int(at - start)])
            }
            Drag::Start(at) => self.run("indent-start", vec![Arg::Int(at)]),
            Drag::End(at) => {
                let extent = Part::ruler(&self.editor).map_or(0, |r| r.extent);
                self.run("indent-end", vec![Arg::Int(extent - at)])
            }
        }
    }

    // ---- drawing

    /// The window as a display list, in layout units from its top left:
    /// 9 525 to a logical pixel.
    pub fn view(&self) -> Page {
        let kit = self.kit();
        // The document's area, which the editor fills with the desk, and
        // the chrome round it fill all there is.
        let mut page = Page::new(self.width, self.height);
        // The document, in its area.
        let area = self.area();
        let look = part::Look {
            caret: self.caret,
            ..crate::editor::view_look(&self.res, self.dark)
        };
        let mut frame = Page::new(area.width, area.height);
        frame.items = self.editor.frame(&look);
        page.place(&frame, Point::new(area.x, area.y));
        // The chrome over it.
        if let Some(ruler) = self.ruler_view() {
            page.items.extend(ruler.draw(&kit));
        }
        let model = self.inspector_model();
        if let Some(view) = self.inspector_view(&model) {
            let hover = match self.hover {
                Hover::Inspector(hit) => Some(hit),
                _ => None,
            };
            let editing = self.editing.as_ref().map(|(at, f)| (*at, f));
            page.items
                .extend(view.draw(&kit, &model, self.tab, hover, editing));
        }
        let message = self.editor.message();
        page.items.extend(toolkit::status_bar(
            &kit,
            &Part::status(&self.editor),
            if message.is_empty() { "Saved" } else { message },
            self.width,
            self.height,
        ));
        let menus = self.menu_bar();
        let buttons = self.buttons();
        let name = self.editor.name();
        let model_title = self.title_model(&menus, &buttons, &name);
        let bar = TitleBar::new(&kit, &model_title, self.width);
        let hover = match self.hover {
            Hover::Title(hit) => Some(hit),
            _ => None,
        };
        page.items.extend(bar.draw(&kit, &model_title, hover));
        page.items.extend(self.draw_menus(&kit, &menus, &bar));
        if let Some((popup, rows)) = self.panel_popup() {
            let lit = self.editor.panel().map(|(_, _, selected)| selected + 2);
            page.items.extend(popup.draw(&kit, &rows, lit, false));
        }
        if let Some((popup, rows)) = self.choice_popup(&model) {
            let lit = self.choosing.and_then(|(_, l)| l);
            page.items.extend(popup.draw(&kit, &rows, lit, false));
        }
        if let (Some(view), Some(palette)) = (self.palette_view(), &self.palette) {
            let first = Window::palette_first(palette);
            page.items.extend(view.draw(
                &kit,
                &palette.field,
                &palette.found,
                first,
                palette.selected,
            ));
        }
        page
    }

    /// Where an input method's candidate window should go: below the caret,
    /// as x, y, width and height in logical pixels.
    pub fn ime_area(&self) -> (f64, f64, f64, f64) {
        let area = self.area();
        let px = |v: i64| v as f64 / PIXEL as f64;
        match Part::caret(&self.editor) {
            Some((x, top, bottom)) => (px(area.x + x), px(area.y + top), 1.0, px(bottom - top)),
            None => (px(area.x), px(area.y), 1.0, 1.0),
        }
    }

    /// The window as screen readers see it: the window, its menus, the
    /// document's text with its lines, words and selection, and the
    /// status bar. Bounds are in logical pixels.
    pub fn access(&self) -> part::accesskit::TreeUpdate {
        use part::accesskit::{Affine, Node, NodeId, Role, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let mut nodes = Vec::new();
        let mut window = Node::new(Role::Window);
        window.set_label(self.title());
        // The menus, each with its commands.
        let bar_id = NodeId(1);
        let mut bar = Node::new(Role::MenuBar);
        let menus = self.menu_bar();
        let mut titles = Vec::new();
        let mut next = 2;
        for menu in &menus.menus {
            let id = NodeId(next);
            next += 1;
            let mut node = Node::new(Role::MenuItem);
            node.set_label(menu.title.clone());
            titles.push(id);
            nodes.push((id, node));
        }
        bar.set_children(titles);
        nodes.push((bar_id, bar));
        let status_id = NodeId(next);
        let mut status = Node::new(Role::Status);
        status.set_value(Part::status(&self.editor));
        nodes.push((status_id, status));
        // The document, from its area, in layout units.
        let mut tree = Part::access(&self.editor, 1 << 20);
        let area = self.area();
        if let Some((_, node)) = tree.nodes.iter_mut().find(|(id, _)| *id == tree.root) {
            let scale = 1.0 / PIXEL as f64;
            node.set_transform(
                Affine::translate((area.x as f64 * scale, area.y as f64 * scale))
                    * Affine::scale(scale),
            );
        }
        window.set_children(vec![bar_id, tree.root, status_id]);
        nodes.append(&mut tree.nodes);
        nodes.push((root, window));
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(root)),
            tree_id: part::accesskit::TreeId::ROOT,
            focus: tree.focus,
        }
    }
}
