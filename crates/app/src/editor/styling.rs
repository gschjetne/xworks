//! A document's own stylesheet, edited (docs/ARCHITECTURE.md, section 6):
//! Clear Formatting, Redefine Style from Selection and New Style from
//! Selection; a style's values, base and name, and the page, set from the
//! inspector; the stylesheet kept as one's own, replaced by another, or
//! updated to a newer version of the one it came from, which is offered,
//! never applied by itself; and new documents in a stylesheet chosen from
//! File.
//!
//! The inspector's Style tab shows the caret's paragraph style: its name
//! and base, each value, and where it comes from, the style itself, a
//! style up its chain, or the defaults, a part it names shown as the
//! value; and the stylesheet it was copied from. Redefining a style from
//! the selection and making one from it are in Format, Styles.

use chrome::{Arg, Choice, Control, ControlKind, InspectorTab, Length, Row, Section, Unit};
use doc::{Document, ObjectId, Op, Props, StyleKind, Value};
use story::styles::{CHARACTER_FORMATTING, Level, PARAGRAPH_FORMATTING, find, valid};
use story::{SheetSpec, keys};
use text::units::MILLIMETRE;

use super::{Editor, Effect, Panel, Pick};

/// Line heights the Style tab offers, in thousandths of the font; 0 is as
/// tall as the fonts make them.
const LINE_HEIGHTS: &[i64] = &[0, 1000, 1150, 1200, 1250, 1350, 1500, 2000];

/// A stylesheet's key in File, New with Stylesheet and Format, Stylesheet,
/// Replace With: one that comes with xWorks, or one of the person's own.
fn key(own: bool, name: &str) -> String {
    format!("{}:{name}", if own { "own" } else { "app" })
}

fn control(command: &str, args: Vec<Arg>, enabled: bool, kind: ControlKind) -> Control {
    Control {
        command: command.to_string(),
        args,
        enabled,
        kind,
    }
}

fn note(text: &str) -> Control {
    control(
        "",
        Vec::new(),
        true,
        ControlKind::Note {
            text: text.to_string(),
        },
    )
}

/// A style's or a part's name.
fn name_of(document: &Document, id: ObjectId) -> String {
    match document.style(id).and_then(|s| s.props.get(keys::NAME)) {
        Some(Value::Text(name)) => name.clone(),
        _ => String::new(),
    }
}

impl Editor {
    /// The stylesheets a document can be made in or take, as (key, name):
    /// those that come with xWorks, then the person's own.
    pub(crate) fn stylesheet_choices(res: &crate::Resources) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = res
            .stylesheets
            .iter()
            .map(|s| (key(false, &s.name), s.name.clone()))
            .collect();
        out.extend(
            res.own
                .list()
                .0
                .into_iter()
                .map(|s| (key(true, &s.spec.name), s.spec.name)),
        );
        out
    }

    /// The stylesheet `key` names, if there is one.
    fn stylesheet_for(&self, key: &str) -> Option<SheetSpec> {
        if let Some(name) = key.strip_prefix("app:") {
            return self
                .res
                .stylesheets
                .iter()
                .find(|s| s.name == name)
                .cloned();
        }
        let name = key.strip_prefix("own:")?;
        self.res.own.get(name).map(|s| s.spec)
    }

    /// A newer version of the stylesheet the document's was copied from,
    /// if there is one: one that comes with xWorks, or one of the person's
    /// own, of its name and a higher version.
    pub(crate) fn newer_stylesheet(&self) -> Option<(String, SheetSpec)> {
        let (name, version) = self.sheet.origin.clone()?;
        let builtin = self
            .res
            .stylesheets
            .iter()
            .find(|s| s.name == name)
            .map(|s| (key(false, &s.name), s.clone()));
        let found = builtin.or_else(|| {
            self.res
                .own
                .get(&name)
                .map(|s| (key(true, &s.spec.name), s.spec))
        })?;
        (found.1.version > version).then_some(found)
    }

    /// Says, in the status bar, that a newer version of the document's
    /// stylesheet can be had, where one can: an offer the person can take
    /// from Format, Stylesheet, or leave.
    pub(crate) fn offer_update(&mut self) {
        if !self.writable() || !self.message.is_empty() {
            return;
        }
        if let Some((_, newer)) = self.newer_stylesheet() {
            self.message = format!(
                "{} has a newer version: Format, Styles, Update Stylesheet shows what changed",
                newer.name
            );
        }
    }

    /// Whether the selection has formatting of its own: its paragraphs'
    /// own paragraph values, or its text's own character values, or with
    /// none selected, what text typed at the caret would take.
    pub(crate) fn has_direct_formatting(&self) -> bool {
        let paragraphs = self.selected_paragraphs();
        if paragraphs.iter().any(|&id| {
            let p = self.paragraph(id);
            PARAGRAPH_FORMATTING
                .iter()
                .any(|k| p.props.contains_key(*k))
        }) {
            return true;
        }
        if self.collapsed() {
            let props = self.typing_props(self.focus);
            return CHARACTER_FORMATTING.iter().any(|k| props.contains_key(*k));
        }
        let (start, end) = self.ordered();
        let last = paragraphs.len().saturating_sub(1);
        paragraphs.iter().enumerate().any(|(i, &id)| {
            let p = self.paragraph(id);
            let from = if i == 0 { start.offset } else { 0 };
            let to = if i == last { end.offset } else { p.len() };
            let mut pos = 0;
            p.runs.iter().any(|run| {
                let (a, b) = (pos, pos + run.len);
                pos = b;
                b > from
                    && a < to
                    && CHARACTER_FORMATTING
                        .iter()
                        .any(|k| run.props.contains_key(*k))
            })
        })
    }

    /// The values the selection sets itself, over its style, as people
    /// name them: its paragraphs' own and its text's own.
    pub(crate) fn own_formatting(&self) -> Vec<&'static str> {
        let name = |key: &str| match key {
            keys::ALIGN => "alignment",
            keys::SPACE_BEFORE | keys::SPACE_AFTER => "spacing",
            keys::LINE_HEIGHT => "line height",
            keys::INDENT_START | keys::INDENT_END | keys::INDENT_FIRST => "indents",
            keys::TABS => "tab stops",
            keys::FONT => "font",
            keys::SIZE => "size",
            keys::COLOR => "colour",
            _ => "underline",
        };
        let mut out: Vec<&'static str> = Vec::new();
        let mut add = |key: &str| {
            let n = name(key);
            if !out.contains(&n) {
                out.push(n);
            }
        };
        let paragraphs = self.selected_paragraphs();
        for &id in &paragraphs {
            let p = self.paragraph(id);
            for key in PARAGRAPH_FORMATTING {
                if p.props.contains_key(key) {
                    add(key);
                }
            }
        }
        let (start, end) = self.ordered();
        let last = paragraphs.len().saturating_sub(1);
        for (i, &id) in paragraphs.iter().enumerate() {
            let p = self.paragraph(id);
            let from = if i == 0 { start.offset } else { 0 };
            let to = if i == last { end.offset } else { p.len() };
            let mut pos = 0;
            for run in &p.runs {
                let (a, b) = (pos, pos + run.len);
                pos = b;
                let touched = if from == to {
                    a <= from && from <= b
                } else {
                    b > from && a < to
                };
                if touched {
                    for key in CHARACTER_FORMATTING {
                        if run.props.contains_key(key) {
                            add(key);
                        }
                    }
                }
            }
        }
        out
    }

    /// Clear Formatting: takes the selection's own formatting away, its
    /// paragraphs' and its text's, so they look as their styles say; with
    /// none selected, the caret's paragraph's, all of it.
    pub(super) fn clear_formatting(&mut self) {
        let paragraphs = self.selected_paragraphs();
        let (start, end) = self.ordered();
        let collapsed = self.collapsed();
        let last = paragraphs.len().saturating_sub(1);
        let mut ops = Vec::new();
        for (i, &id) in paragraphs.iter().enumerate() {
            let len = self.paragraph(id).len();
            let range = if collapsed {
                None
            } else {
                Some((
                    if i == 0 { start.offset } else { 0 },
                    if i == last { end.offset } else { len },
                ))
            };
            // Paragraph formatting goes from every paragraph touched.
            ops.extend(self.clearing(id, None, false));
            ops.extend(
                self.clearing(id, range.or(Some((0, len))), true)
                    .into_iter()
                    .filter(|op| matches!(op, Op::SetCharProp { .. })),
            );
        }
        if collapsed {
            for key in CHARACTER_FORMATTING {
                self.pending.remove(key);
            }
        }
        if ops.is_empty() {
            self.message = "The selection has no formatting of its own".to_string();
            return;
        }
        self.commit("Clear Formatting", ops, false);
    }

    /// The selection's own formatting as a style's values: the caret's
    /// paragraph's own paragraph values, and the character values every
    /// run of it sets alike.
    fn selection_values(&self) -> Props {
        let p = self.paragraph(self.focus.paragraph);
        let mut values: Props = PARAGRAPH_FORMATTING
            .iter()
            .filter_map(|&k| Some((k.to_string(), p.props.get(k)?.clone())))
            .collect();
        for key in CHARACTER_FORMATTING {
            let mut seen = p.runs.iter().map(|r| r.props.get(key));
            if let Some(Some(first)) = seen.next()
                && seen.all(|v| v == Some(first))
            {
                values.insert(key.to_string(), first.clone());
            }
        }
        values
    }

    /// The operations that take `values` away from the selected
    /// paragraphs of style `style` that set them alike, now that their
    /// style does.
    fn absorbing(&self, style: Option<ObjectId>, values: &Props) -> Vec<Op> {
        let mut ops = Vec::new();
        for id in self.selected_paragraphs() {
            let p = self.paragraph(id);
            if self.sheet.style_of(p).id != style {
                continue;
            }
            for (key, value) in values {
                if PARAGRAPH_FORMATTING.contains(&key.as_str()) && p.props.get(key) == Some(value) {
                    ops.push(Op::SetParaProp {
                        paragraph: id,
                        key: key.clone(),
                        value: None,
                    });
                } else if CHARACTER_FORMATTING.contains(&key.as_str()) && !p.is_empty() {
                    ops.push(Op::SetCharProp {
                        paragraph: id,
                        offset: 0,
                        len: p.len(),
                        key: key.clone(),
                        value: None,
                    });
                }
            }
        }
        ops
    }

    /// Redefine Style from Selection: the caret's paragraph style takes
    /// the selection's own formatting as its values, which the selected
    /// paragraphs of that style then no longer set themselves. A style
    /// changes only when it's edited, and this is an edit (project owner,
    /// 2026-10-24, review 30dmn1js).
    pub(super) fn redefine_style(&mut self) {
        let style = self.sheet.style_of(self.paragraph(self.focus.paragraph));
        let (Some(id), name) = (style.id, style.name.clone()) else {
            self.message = "The paragraph has no style to redefine".to_string();
            return;
        };
        let values = self.selection_values();
        if values.is_empty() {
            self.message = format!("The selection has nothing {name} doesn't already");
            return;
        }
        let mut ops: Vec<Op> = values
            .iter()
            .map(|(key, value)| Op::SetStyleProp {
                style: id,
                key: key.clone(),
                value: Some(value.clone()),
            })
            .collect();
        ops.extend(self.absorbing(Some(id), &values));
        if self.commit(&format!("Redefine {name}"), ops, false) {
            self.message = format!("Redefined {name} from the selection");
        }
    }

    /// A name for a new paragraph style that no style of the sheet has:
    /// "New Style", or "New Style 2" and so on.
    fn new_style_name(&self) -> String {
        let taken = |name: &str| self.sheet.paragraph.iter().any(|s| s.name == name);
        let mut n = 1;
        loop {
            let name = if n == 1 {
                "New Style".to_string()
            } else {
                format!("New Style {n}")
            };
            if !taken(&name) {
                return name;
            }
            n += 1;
        }
    }

    /// New Style from Selection: a paragraph style based on the caret's
    /// paragraph's, with the selection's own formatting as its values,
    /// which the selected paragraphs then take. It's named "New Style",
    /// to rename in the inspector's Style tab.
    pub(super) fn new_style(&mut self) {
        let Some(sheet) = self.document.sheet() else {
            self.message = "The document has no stylesheet to add a style to".to_string();
            return;
        };
        let base = self.sheet.style_of(self.paragraph(self.focus.paragraph)).id;
        let name = self.new_style_name();
        let values = self.selection_values();
        let mut props = values.clone();
        props.insert(keys::NAME.to_string(), Value::Text(name.clone()));
        let mut scratch = self.document.clone();
        let id = scratch.new_id();
        let after = base.filter(|b| self.document.styles().contains(b)).or(self
            .document
            .styles()
            .last()
            .copied());
        let mut ops = vec![Op::InsertStyle {
            sheet,
            after,
            id,
            style: doc::Style {
                sheet,
                kind: StyleKind::Paragraph,
                base,
                props,
            },
        }];
        ops.extend(self.absorbing(base, &values));
        for paragraph in self.selected_paragraphs() {
            ops.push(Op::SetParaProp {
                paragraph,
                key: keys::STYLE.to_string(),
                value: Some(Value::Ref(id)),
            });
        }
        if self.commit("New Style", ops, false) {
            self.message = format!("Made {name} from the selection: rename it in the Style tab");
        }
    }

    /// Keeps the document's stylesheet as one of the person's own, named
    /// after the sheet it came from, or "My" and that name if it came with
    /// xWorks, so a sheet that comes with xWorks keeps its name.
    pub(super) fn save_stylesheet(&mut self) {
        let Some(mut spec) = SheetSpec::of(&self.document) else {
            self.message = "The document's stylesheet can't be kept as it is".to_string();
            return;
        };
        if self.res.stylesheets.iter().any(|s| s.name == spec.name) {
            spec.name = format!("My {}", spec.name);
            spec.version = 1;
        }
        match self.res.own.save(&spec) {
            Ok(_) => {
                self.message = format!("Kept the stylesheet as your own: {}", spec.name);
                // The document now came from it.
                let saved = self.res.own.get(&spec.name);
                if let (Some(saved), Some(sheet)) = (saved, self.document.sheet()) {
                    let ops = vec![
                        Op::SetStyleProp {
                            style: sheet,
                            key: keys::ORIGIN.to_string(),
                            value: Some(Value::Text(saved.spec.name)),
                        },
                        Op::SetStyleProp {
                            style: sheet,
                            key: keys::ORIGIN_VERSION.to_string(),
                            value: Some(Value::Int(i64::from(saved.spec.version))),
                        },
                    ];
                    let message = self.message.clone();
                    if self.commit("Keep Stylesheet", ops, false) {
                        self.message = message;
                    }
                }
            }
            Err(e) => self.message = format!("Couldn't keep the stylesheet: {e}"),
        }
    }

    /// Replaces the document's stylesheet with the one `key` names, its
    /// styles matched by role and name with the document's, as pasting
    /// matches them; the document's own styles stay (project owner,
    /// 2026-10-24, review 30dmn1js). One undoable step.
    pub(super) fn replace_stylesheet(&mut self, key: &str) {
        let Some(spec) = self.stylesheet_for(key) else {
            self.message = "There's no such stylesheet".to_string();
            return;
        };
        self.take_stylesheet(&spec, &format!("Stylesheet {}", spec.name));
    }

    /// Makes the document's stylesheet `spec`'s, as [`Editor::replace_stylesheet`]
    /// does, as one step named `name`.
    fn take_stylesheet(&mut self, spec: &SheetSpec, name: &str) {
        let incoming = story::new_document(spec);
        let mut scratch = self.document.clone();
        let (ops, _) = story::merge(&mut scratch, &incoming, true);
        if ops.is_empty() {
            self.message = format!("The document is in {} already", spec.name);
            return;
        }
        self.commit(name, ops, false);
    }

    /// Format, Stylesheet, Update: what the newer version of the
    /// document's stylesheet would change, to take or leave.
    pub(super) fn update_panel(&mut self) {
        let Some((key, newer)) = self.newer_stylesheet() else {
            self.message = "The stylesheet is as new as the one it came from".to_string();
            return;
        };
        let incoming = story::new_document(&newer);
        let changes = story::changes(&self.document, &incoming);
        let mut rows = vec![
            (
                format!("Take {}'s version {}", newer.name, newer.version),
                Pick::Stylesheet(key),
            ),
            ("Leave the styles as they are".to_string(), Pick::Nothing),
        ];
        rows.extend(
            changes
                .into_iter()
                .take(40)
                .map(|c| (format!("    {}: {}", c.style, c.what), Pick::Nothing)),
        );
        self.panel = Some(Panel {
            title: format!("{} has changed since the document copied it", newer.name),
            rows,
            selected: 0,
        });
    }

    /// Takes the stylesheet `key` names as a newer version of the
    /// document's, chosen from the update's panel.
    pub(super) fn take_update(&mut self, key: &str) {
        if let Some(spec) = self.stylesheet_for(key) {
            self.take_stylesheet(&spec, &format!("Update to {} {}", spec.name, spec.version));
        }
    }

    /// File, New with Stylesheet: a new document in the stylesheet `key`
    /// names, made at once in the documents folder and opened.
    pub(super) fn new_with_stylesheet(&mut self, key: &str) -> Vec<Effect> {
        let Some(spec) = self.stylesheet_for(key) else {
            self.message = "There's no such stylesheet".to_string();
            return Vec::new();
        };
        match Editor::create_in(self.res.clone(), &spec) {
            Ok(editor) => {
                let path = editor.path().to_path_buf();
                drop(editor);
                vec![Effect::Open(Some(path))]
            }
            Err(e) => {
                self.message = format!("Couldn't make a document: {e}");
                Vec::new()
            }
        }
    }

    /// Runs one of the commands that edit a style or the page, with
    /// `args`, as the inspector gives them; false if `id` isn't one.
    pub(super) fn style_command(&mut self, id: &str, args: &[Arg]) -> bool {
        let style = || {
            args.first()
                .and_then(Arg::int)
                .and_then(|n| u64::try_from(n).ok())
                .map(ObjectId)
        };
        let text = |i: usize| args.get(i).and_then(Arg::text);
        match id {
            "style-set" => {
                if let (Some(style), Some(key), Some(value)) = (style(), text(1), args.get(2)) {
                    self.set_style_value(style, key, value);
                }
            }
            "style-toggle" => {
                if let (Some(style), Some(key)) = (style(), text(1)) {
                    let on = matches!(
                        find(&self.document, Some(style), key, true, &valid(key)),
                        Some((Value::Bool(true), _))
                    );
                    self.set_style_value(style, key, &Arg::Int(i64::from(!on)));
                }
            }
            "style-reset" => {
                if let (Some(style), Some(key)) = (style(), text(1)) {
                    let name = name_of(&self.document, style);
                    self.commit(
                        &format!("Edit {name}"),
                        vec![Op::SetStyleProp {
                            style,
                            key: key.to_string(),
                            value: None,
                        }],
                        false,
                    );
                }
            }
            "style-base" => {
                if let (Some(style), Some(base)) = (style(), args.get(1).and_then(Arg::int)) {
                    let base = u64::try_from(base).ok().filter(|&b| b > 0).map(ObjectId);
                    let name = name_of(&self.document, style);
                    if !self.commit(
                        &format!("Base {name}"),
                        vec![Op::SetBase { style, base }],
                        false,
                    ) {
                        self.message =
                            format!("{name} can't be based on that: it would be based on itself");
                    }
                }
            }
            "style-rename" => {
                if let (Some(style), Some(name)) = (style(), text(1)) {
                    self.rename_style(style, name.trim());
                }
            }
            "page-size" => {
                if let Some(name) = text(0)
                    && let Some((_, w, h)) = PAGE_SIZES.iter().find(|(n, ..)| *n == name)
                {
                    self.set_page(&[(keys::PAGE_WIDTH, *w), (keys::PAGE_HEIGHT, *h)]);
                }
            }
            "page-margin" => {
                if let (Some(side), Some(value)) = (text(0), args.get(1).and_then(Arg::int)) {
                    let key = match side {
                        "top" => keys::MARGIN_TOP,
                        "bottom" => keys::MARGIN_BOTTOM,
                        "left" => keys::MARGIN_LEFT,
                        "right" => keys::MARGIN_RIGHT,
                        _ => return true,
                    };
                    self.set_page(&[(key, value)]);
                }
            }
            _ => return false,
        }
        true
    }

    /// Sets `style`'s value of `key` to `value`, as the inspector gives
    /// it: a number, which for a face, a colour or a tab layout names the
    /// part, 0 none; or text.
    fn set_style_value(&mut self, style: ObjectId, key: &str, value: &Arg) {
        let refers = [keys::FONT, keys::COLOR, keys::TABS].contains(&key);
        let value = match value {
            Arg::Int(n) if refers => u64::try_from(*n)
                .ok()
                .filter(|&n| n > 0)
                .map(|n| Value::Ref(ObjectId(n))),
            Arg::Int(n)
                if matches!(
                    key,
                    keys::BOLD | keys::ITALIC | keys::UNDERLINE | keys::KEEP_WITH_NEXT
                ) =>
            {
                Some(Value::Bool(*n != 0))
            }
            Arg::Int(n) => Some(Value::Int(*n)),
            Arg::Text(t) => Some(Value::Text(t.clone())),
        };
        if let Some(v) = &value
            && !matches!(v, Value::Ref(_))
            && !valid(key)(v)
        {
            self.message = format!("That isn't a {key} a style can have");
            return;
        }
        let name = name_of(&self.document, style);
        self.commit(
            &format!("Edit {name}"),
            vec![Op::SetStyleProp {
                style,
                key: key.to_string(),
                value,
            }],
            false,
        );
    }

    /// Renames `style` `name`, one edit that paragraphs never notice, as
    /// they name it by ID; a name another style of its kind has is
    /// refused.
    fn rename_style(&mut self, style: ObjectId, name: &str) {
        let Some(kind) = self.document.style(style).map(|s| s.kind) else {
            return;
        };
        if name.is_empty() || name.chars().count() > 100 {
            self.message = "A style's name is 1 to 100 characters".to_string();
            return;
        }
        let taken = self.document.styles().iter().any(|&other| {
            other != style
                && self.document.style(other).is_some_and(|s| s.kind == kind)
                && name_of(&self.document, other) == name
        });
        if taken {
            self.message = format!("Another style is called {name}");
            return;
        }
        let old = name_of(&self.document, style);
        self.commit(
            &format!("Rename {old}"),
            vec![Op::SetStyleProp {
                style,
                key: keys::NAME.to_string(),
                value: Some(Value::Text(name.to_string())),
            }],
            false,
        );
    }

    /// The document's page style, its first.
    fn page_style_id(&self) -> Option<ObjectId> {
        self.document.styles().iter().copied().find(|&id| {
            self.document
                .style(id)
                .is_some_and(|s| s.kind == StyleKind::Page)
        })
    }

    /// Sets the page style's `values`, where they leave room on the page.
    fn set_page(&mut self, values: &[(&str, i64)]) {
        let Some(page) = self.page_style_id() else {
            self.message = "The document has no page style".to_string();
            return;
        };
        let mut check = self.sheet.page;
        for (key, value) in values {
            match *key {
                keys::PAGE_WIDTH => check.width = *value,
                keys::PAGE_HEIGHT => check.height = *value,
                keys::MARGIN_TOP => check.top = *value,
                keys::MARGIN_BOTTOM => check.bottom = *value,
                keys::MARGIN_LEFT => check.left = *value,
                _ => check.right = *value,
            }
        }
        let fits = values.iter().all(|(k, v)| valid(k)(&Value::Int(*v)));
        if !fits || check.body_width() < 10 * MILLIMETRE || check.body_height() < 10 * MILLIMETRE {
            self.message = "That leaves no room on the page".to_string();
            return;
        }
        let ops = values
            .iter()
            .map(|(key, value)| Op::SetStyleProp {
                style: page,
                key: key.to_string(),
                value: Some(Value::Int(*value)),
            })
            .collect();
        self.commit("Page", ops, false);
    }

    /// Where `style`'s value of `key` comes from, as the Style tab notes
    /// it: "this style", the style up its chain that sets it, or the
    /// defaults.
    fn source_note(&self, style: ObjectId, key: &str) -> (String, bool) {
        match find(&self.document, Some(style), key, true, &valid(key)) {
            Some((_, source)) => match source.level {
                Level::Style(s) if s == style => ("this style".to_string(), true),
                Level::Style(s) => (name_of(&self.document, s), false),
                Level::Defaults => ("defaults".to_string(), false),
                Level::Direct | Level::Program => ("xWorks".to_string(), false),
            },
            None => ("xWorks".to_string(), false),
        }
    }

    /// The inspector's Style tab: the caret's paragraph style, its name
    /// and base, each of its values with where it comes from, and the
    /// stylesheet's own commands.
    pub(super) fn style_tab(&self) -> InspectorTab {
        let writable = self.writable();
        let unit = self.res.settings.units();
        let style = self
            .sheet
            .style_of(self.paragraph(self.focus.paragraph))
            .clone();
        let mut sections = Vec::new();
        let Some(id) = style.id else {
            sections.push(Section {
                title: "Paragraph style".to_string(),
                rows: vec![Row {
                    label: String::new(),
                    controls: vec![note("The document's stylesheet has no paragraph styles")],
                }],
            });
            return InspectorTab {
                title: "Style".to_string(),
                sections,
            };
        };
        let sid = Arg::Int(id.0 as i64);
        let arg = |key: &str| vec![sid.clone(), Arg::Text(key.to_string())];
        // A value's row: its control, where it comes from, and a button
        // to let the style take it from up its chain again, where it sets
        // it itself.
        let row = |label: &str, key: &str, kind: ControlKind| -> Row {
            let (source, own) = self.source_note(id, key);
            let mut controls = vec![
                control("style-set", arg(key), writable, kind),
                note(&source),
            ];
            if own {
                controls.push(control(
                    "style-reset",
                    arg(key),
                    writable,
                    ControlKind::Button {
                        label: "Take it from up the chain".to_string(),
                        icon: Some("x".to_string()),
                    },
                ));
            }
            Row {
                label: label.to_string(),
                controls,
            }
        };
        // The style itself: its name, and what it's based on.
        let bases: Vec<&story::ParagraphStyle> = self
            .sheet
            .paragraph
            .iter()
            .filter(|s| {
                s.id.is_some_and(|b| b != id && !self.document.chain(b).contains(&id))
            })
            .collect();
        let mut base_choices = vec![Choice {
            value: Arg::Int(0),
            label: "Nothing".to_string(),
        }];
        base_choices.extend(bases.iter().map(|s| Choice {
            value: Arg::Int(s.id.map_or(0, |b| b.0 as i64)),
            label: s.name.clone(),
        }));
        let base_selected = match style.base {
            None => Some(0),
            Some(b) => bases.iter().position(|s| s.id == Some(b)).map(|i| i + 1),
        };
        sections.push(Section {
            title: "Paragraph style".to_string(),
            rows: vec![
                Row {
                    label: "Name".to_string(),
                    controls: vec![control(
                        "style-rename",
                        vec![sid.clone()],
                        writable,
                        ControlKind::Text {
                            value: style.name.clone(),
                        },
                    )],
                },
                Row {
                    label: "Based on".to_string(),
                    controls: vec![control(
                        "style-base",
                        vec![sid.clone()],
                        writable,
                        ControlKind::Choice {
                            choices: base_choices,
                            selected: base_selected,
                            plain: false,
                        },
                    )],
                },
            ],
        });
        // What the selection sets itself, over its style: its own
        // formatting, to clear.
        let own = self.own_formatting();
        if !own.is_empty() {
            sections[0].rows.push(Row {
                label: "Its own".to_string(),
                controls: vec![
                    note(&own.join(", ")),
                    control(
                        "clear-formatting",
                        Vec::new(),
                        writable,
                        ControlKind::Button {
                            label: "Clear Formatting".to_string(),
                            icon: Some("x".to_string()),
                        },
                    ),
                ],
            });
        }
        // Its text: face, size, weight and slant, colour.
        let parts = |kind: StyleKind| -> Vec<&story::Part> {
            self.sheet.parts.iter().filter(|p| p.kind == kind).collect()
        };
        let own_ref = |key: &str| match find(&self.document, Some(id), key, true, &|_| true) {
            Some((_, source)) => source.part,
            None => None,
        };
        let faces = parts(StyleKind::Face);
        let face = own_ref(keys::FONT);
        let mut face_choices: Vec<Choice> = faces
            .iter()
            .map(|p| Choice {
                value: Arg::Int(p.id.0 as i64),
                label: p.name.clone(),
            })
            .collect();
        let mut face_selected = face.and_then(|f| faces.iter().position(|p| p.id == f));
        if face_selected.is_none() {
            face_choices.push(Choice {
                value: Arg::Text(style.family.clone()),
                label: style.family.clone(),
            });
            face_selected = Some(face_choices.len() - 1);
        }
        let colours = parts(StyleKind::Colour);
        let colour = own_ref(keys::COLOR);
        let mut colour_choices: Vec<Choice> = colours
            .iter()
            .map(|p| Choice {
                value: Arg::Int(p.id.0 as i64),
                label: p.name.clone(),
            })
            .collect();
        let mut colour_selected = colour.and_then(|c| colours.iter().position(|p| p.id == c));
        if colour_selected.is_none() {
            let (r, g, b) = style.color;
            let hex = format!("#{r:02x}{g:02x}{b:02x}");
            colour_choices.push(Choice {
                value: Arg::Text(hex.clone()),
                label: hex,
            });
            colour_selected = Some(colour_choices.len() - 1);
        }
        let toggle = |key: &str, icon: &str, label: &str, on: bool| {
            control(
                "style-toggle",
                arg(key),
                writable,
                ControlKind::Toggle {
                    icon: icon.to_string(),
                    label: label.to_string(),
                    on,
                },
            )
        };
        let (weight_source, _) = self.source_note(id, keys::BOLD);
        sections.push(Section {
            title: "Text".to_string(),
            rows: vec![
                row(
                    "Face",
                    keys::FONT,
                    ControlKind::Choice {
                        choices: face_choices,
                        selected: face_selected,
                        plain: false,
                    },
                ),
                row(
                    "Size",
                    keys::SIZE,
                    ControlKind::Length {
                        value: Some(Length {
                            value: style.size,
                            unit: Unit::Point,
                        }),
                        prefix: String::new(),
                        plain: false,
                    },
                ),
                Row {
                    label: "Weight".to_string(),
                    controls: vec![
                        toggle(keys::BOLD, "bold", "Bold", style.bold),
                        toggle(keys::ITALIC, "italic", "Italic", style.italic),
                        note(&weight_source),
                    ],
                },
                row(
                    "Colour",
                    keys::COLOR,
                    ControlKind::Choice {
                        choices: colour_choices,
                        selected: colour_selected,
                        plain: false,
                    },
                ),
            ],
        });
        // Its paragraphs: alignment, spacing, line height, indents, tabs.
        let aligns = ["start", "center", "end", "justify"];
        let align_labels = ["Start", "Centre", "End", "Justify"];
        let length = |value: i64, unit: Unit| ControlKind::Length {
            value: Some(Length { value, unit }),
            prefix: String::new(),
            plain: false,
        };
        let mut heights = LINE_HEIGHTS.to_vec();
        let line = style.line_height.unwrap_or(0);
        if !heights.contains(&line) {
            heights.push(line);
            heights.sort_unstable();
        }
        let layouts = parts(StyleKind::Tabs);
        let layout = own_ref(keys::TABS);
        let mut tab_choices = vec![Choice {
            value: Arg::Int(0),
            label: if style.tabs.is_empty() {
                "None".to_string()
            } else {
                "Its own".to_string()
            },
        }];
        tab_choices.extend(layouts.iter().map(|p| Choice {
            value: Arg::Int(p.id.0 as i64),
            label: p.name.clone(),
        }));
        let tab_selected = match layout {
            Some(l) => layouts.iter().position(|p| p.id == l).map(|i| i + 1),
            None => Some(0),
        };
        sections.push(Section {
            title: "Paragraph".to_string(),
            rows: vec![
                row(
                    "Align",
                    keys::ALIGN,
                    ControlKind::Choice {
                        choices: aligns
                            .iter()
                            .zip(align_labels)
                            .map(|(a, label)| Choice {
                                value: Arg::Text(a.to_string()),
                                label: label.to_string(),
                            })
                            .collect(),
                        selected: aligns
                            .iter()
                            .position(|a| *a == story::sheet::align_name(style.align)),
                        plain: false,
                    },
                ),
                row(
                    "Above",
                    keys::SPACE_BEFORE,
                    length(style.space_before, Unit::Point),
                ),
                row(
                    "Below",
                    keys::SPACE_AFTER,
                    length(style.space_after, Unit::Point),
                ),
                row(
                    "Line",
                    keys::LINE_HEIGHT,
                    ControlKind::Choice {
                        selected: heights.iter().position(|&h| h == line),
                        choices: heights
                            .iter()
                            .map(|&h| Choice {
                                value: Arg::Int(h),
                                label: super::format::line_height_label(h),
                            })
                            .collect(),
                        plain: false,
                    },
                ),
                row(
                    "Indent",
                    keys::INDENT_START,
                    length(style.indents.start, unit),
                ),
                row(
                    "First line",
                    keys::INDENT_FIRST,
                    length(style.indents.first, unit),
                ),
                row(
                    "Tab stops",
                    keys::TABS,
                    ControlKind::Choice {
                        choices: tab_choices,
                        selected: tab_selected,
                        plain: false,
                    },
                ),
            ],
        });
        // The stylesheet: where it came from, and what can be done with it.
        let origin = match &self.sheet.origin {
            Some((name, version)) => format!("{name}, version {version}"),
            None => "its own".to_string(),
        };
        let mut rows = vec![
            Row {
                label: "Copied from".to_string(),
                controls: vec![note(&origin)],
            },
            Row {
                label: String::new(),
                controls: vec![control(
                    "save-stylesheet",
                    Vec::new(),
                    writable,
                    ControlKind::Button {
                        label: "Keep as my own".to_string(),
                        icon: None,
                    },
                )],
            },
        ];
        if let Some((_, newer)) = self.newer_stylesheet() {
            rows.push(Row {
                label: String::new(),
                controls: vec![control(
                    "update-stylesheet",
                    Vec::new(),
                    writable,
                    ControlKind::Button {
                        label: format!("See what's new in {} {}", newer.name, newer.version),
                        icon: None,
                    },
                )],
            });
        }
        sections.push(Section {
            title: "Stylesheet".to_string(),
            rows,
        });
        InspectorTab {
            title: "Style".to_string(),
            sections,
        }
    }
}

/// The page sizes the Page tab offers, with their widths and heights.
pub(super) const PAGE_SIZES: [(&str, i64, i64); 4] = [
    ("A4", 210 * MILLIMETRE, 297 * MILLIMETRE),
    ("A5", 148 * MILLIMETRE, 210 * MILLIMETRE),
    ("US Letter", 2159 * MILLIMETRE / 10, 2794 * MILLIMETRE / 10),
    ("US Legal", 2159 * MILLIMETRE / 10, 3556 * MILLIMETRE / 10),
];
