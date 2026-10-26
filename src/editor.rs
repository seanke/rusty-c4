//! The source panel: a file tree and a code editor on the right, editing the project's files
//! in place. Problems get squiggly underlines, ⌘. applies a suggested fix, and an
//! autocomplete popup offers keywords, kinds and element names as you type. The editing
//! itself (cursor, selection, wrapping, hit-testing) is cosmic-text's `Editor`; this module
//! draws it into an image shown in the panel.

use std::collections::BTreeSet;

use bevy::{
    asset::RenderAssetUsages,
    ecs::system::NonSendMarker,
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::{AccumulatedMouseScroll, MouseScrollUnit},
        ButtonState,
    },
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui::RelativeCursorPosition,
};
use cosmic_text::{
    fontdb, Action, Align, Attrs, AttrsList, Buffer, Color as Ink, Cursor, Edit, Editor, Family, FontSystem, Metrics,
    Motion, Selection, Shaping, SwashCache, Wrap,
};

use crate::parser::{is_mermaid, Diagnostic};
use crate::scene::{Diagram, Fonts, Watch, TEXT};
use crate::ui::WhenLoaded;
use crate::workspace::Workspace;

/// Panel width, in logical pixels.
pub const PANEL_W: f32 = 540.0;
const FONT: f32 = 13.0;
const LINE: f32 = 19.0;
const GUTTER: f32 = 44.0;
const PAD: f32 = 8.0;

const BG: [u8; 4] = [14, 17, 23, 255];
const LINE_BG: [u8; 4] = [255, 255, 255, 10];
const SELECTION: [u8; 4] = [70, 130, 210, 90];
const CARET: [u8; 4] = [120, 200, 255, 255];
const GUTTER_INK: Ink = Ink::rgba(110, 120, 140, 255);
const ERROR: [u8; 4] = [240, 90, 90, 255];
const WARNING: [u8; 4] = [235, 185, 70, 255];

#[derive(Clone, Copy, PartialEq)]
enum Lang {
    C4,
    Mermaid,
}

const C4_KEYWORDS: &[&str] = &[
    "specification", "model", "views", "view", "dynamic", "element", "relationship", "tag", "color", "style",
    "shape", "include", "exclude", "extend", "group", "parallel", "title", "description", "technology",
    "summary", "link", "icon", "metadata", "notation", "navigateTo", "autoLayout", "deployment", "global", "this",
    "it", "of", "person", "rectangle", "storage", "cylinder", "queue", "browser", "mobile",
];
const MERMAID_KEYWORDS: &[&str] = &[
    "flowchart", "graph", "subgraph", "end", "direction", "classDef", "class", "style", "linkStyle", "click",
    "sequenceDiagram", "participant", "actor", "as", "loop", "alt", "else", "opt", "par", "and", "critical",
    "break", "rect", "Note", "over", "title", "activate", "deactivate", "autonumber", "TD", "TB", "LR", "RL", "BT",
];

#[derive(Clone, Copy, PartialEq)]
enum Token {
    Plain,
    Keyword,
    Str,
    Comment,
    Arrow,
    Tag,
    Number,
    Brace,
}

fn ink(t: Token) -> Ink {
    match t {
        Token::Plain => Ink::rgb(214, 222, 235),
        Token::Keyword => Ink::rgb(199, 146, 234),
        Token::Str => Ink::rgb(236, 196, 141),
        Token::Comment => Ink::rgb(105, 120, 120),
        Token::Arrow => Ink::rgb(127, 219, 202),
        Token::Tag => Ink::rgb(173, 219, 103),
        Token::Number => Ink::rgb(247, 140, 108),
        Token::Brace => Ink::rgb(140, 150, 170),
    }
}

/// Split a line into coloured byte ranges.
fn highlight(line: &str, lang: Lang) -> Vec<(std::ops::Range<usize>, Token)> {
    let keywords = match lang {
        Lang::C4 => C4_KEYWORDS,
        Lang::Mermaid => MERMAID_KEYWORDS,
    };
    let comment = match lang {
        Lang::C4 => "//",
        Lang::Mermaid => "%%",
    };
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let end_of = |k: usize| chars.get(k).map_or(line.len(), |c| c.0);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        if line[start..].starts_with(comment) {
            out.push((start..line.len(), Token::Comment));
            break;
        }
        let mut j = i + 1;
        let tok = if c == '\'' || c == '"' {
            while j < chars.len() && chars[j].1 != c {
                j += 1;
            }
            j = (j + 1).min(chars.len());
            Token::Str
        } else if c == '#' {
            while j < chars.len() && (chars[j].1.is_alphanumeric() || matches!(chars[j].1, '_' | '-')) {
                j += 1;
            }
            if lang == Lang::C4 { Token::Tag } else { Token::Number }
        } else if "-=<>.~|".contains(c) && chars.get(i + 1).is_some_and(|n| "-=<>.~|[".contains(n.1)) {
            while j < chars.len() && "-=<>.~xo".contains(chars[j].1) {
                j += 1;
            }
            Token::Arrow
        } else if c.is_ascii_digit() {
            while j < chars.len() && (chars[j].1.is_ascii_alphanumeric() || chars[j].1 == '.') {
                j += 1;
            }
            Token::Number
        } else if c.is_alphabetic() || c == '_' {
            while j < chars.len() && (chars[j].1.is_alphanumeric() || chars[j].1 == '_') {
                j += 1;
            }
            if keywords.contains(&&line[start..end_of(j)]) { Token::Keyword } else { Token::Plain }
        } else if "{}[]()".contains(c) {
            Token::Brace
        } else {
            Token::Plain
        };
        if tok != Token::Plain {
            out.push((start..end_of(j), tok));
        }
        i = j;
    }
    out
}

struct Completion {
    items: Vec<String>,
    selected: usize,
    /// Byte where the word being completed starts, on the cursor's line.
    start: usize,
}

struct Snapshot {
    text: String,
    cursor: Cursor,
}

#[derive(Resource)]
pub struct Code {
    fonts: FontSystem,
    cache: SwashCache,
    /// The code font's family name (leaked once, so text attributes can borrow it freely).
    family: Option<&'static str>,
    editor: Editor<'static>,
    gutter: Buffer,
    popup: Buffer,
    pub open: bool,
    pub focused: bool,
    /// Escape was used to leave the editor this frame (so it shouldn't open the menu too).
    swallowed: bool,
    /// What is loaded: (workspace generation, file index, file revision).
    loaded: Option<(u64, usize, u64)>,
    lang: Lang,
    /// Image size in physical pixels, and the window's scale factor.
    size: UVec2,
    sf: f32,
    image: Handle<Image>,
    redraw: bool,
    blink: f32,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: f64,
    completion: Option<Completion>,
    dragging: bool,
    last_click: (f64, Vec2, u8),
    note: Option<(String, f32)>,
    /// Set by the script runner: offer completions / apply a fix on the next update.
    pending_complete: bool,
    pending_fix: bool,
    pending_save: bool,
}

impl Code {
    /// Keys are going to the editor; the rest of the app should leave them alone.
    pub fn captures_keys(&self) -> bool {
        (self.open && self.focused) || self.swallowed
    }

    fn attrs(&self) -> Attrs<'static> {
        let family = match self.family {
            Some(f) => Family::Name(f),
            None => Family::Monospace,
        };
        Attrs::new().family(family).color(ink(Token::Plain))
    }

    fn text(&self) -> String {
        self.editor.with_buffer(|b| {
            let mut s = String::new();
            for l in &b.lines {
                s.push_str(l.text());
                s.push_str(l.ending().as_str());
            }
            s
        })
    }

    fn set_text(&mut self, text: &str) {
        let attrs = self.attrs().clone();
        let fonts = &mut self.fonts;
        self.editor.with_buffer_mut(|b| b.set_text(fonts, text, &attrs, Shaping::Advanced, None));
        self.recolour();
    }

    /// Re-apply syntax colours (only lines whose colours changed are reshaped).
    fn recolour(&mut self) {
        let base = self.attrs().clone();
        let lang = self.lang;
        self.editor.with_buffer_mut(|b| {
            for line in b.lines.iter_mut() {
                let mut list = AttrsList::new(&base);
                for (range, tok) in highlight(line.text(), lang) {
                    list.add_span(range, &base.clone().color(ink(tok)));
                }
                line.set_attrs_list(list);
            }
        });
        self.redraw = true;
    }

    fn snapshot(&mut self) {
        let snap = Snapshot {
            text: self.text(),
            cursor: self.editor.cursor(),
        };
        self.undo.push(snap);
        if self.undo.len() > 300 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(&mut self, snap: Snapshot) {
        self.set_text(&snap.text);
        let lines = self.editor.with_buffer(|b| b.lines.len());
        let mut c = snap.cursor;
        c.line = c.line.min(lines.saturating_sub(1));
        let len = self.editor.with_buffer(|b| b.lines[c.line].text().len());
        c.index = c.index.min(len);
        self.editor.set_cursor(c);
        self.editor.set_selection(Selection::None);
    }

    fn action(&mut self, a: Action) {
        self.editor.action(&mut self.fonts, a);
        self.redraw = true;
        self.blink = 0.0;
    }

    fn motion(&mut self, m: Motion, shift: bool) {
        self.completion = None;
        let sel = self.editor.selection();
        if shift {
            if sel == Selection::None {
                self.editor.set_selection(Selection::Normal(self.editor.cursor()));
            }
        } else if sel != Selection::None {
            // Collapse the selection toward the direction of travel.
            if let Some((a, b)) = self.editor.selection_bounds() {
                if matches!(m, Motion::Left | Motion::Previous) {
                    self.editor.set_cursor(a);
                    self.editor.set_selection(Selection::None);
                    self.redraw = true;
                    return;
                }
                if matches!(m, Motion::Right | Motion::Next) {
                    self.editor.set_cursor(b);
                    self.editor.set_selection(Selection::None);
                    self.redraw = true;
                    return;
                }
            }
            self.editor.set_selection(Selection::None);
        }
        self.action(Action::Motion(m));
    }

    fn line_text(&self, line: usize) -> String {
        self.editor.with_buffer(|b| b.lines.get(line).map(|l| l.text().to_string()).unwrap_or_default())
    }

    /// Replace bytes `start..end` of `line` with `with`, leaving the cursor after it.
    fn replace(&mut self, line: usize, start: usize, end: usize, with: &str) {
        self.editor.set_cursor(Cursor::new(line, end));
        self.editor.set_selection(Selection::Normal(Cursor::new(line, start)));
        self.editor.insert_string(with, None);
        self.redraw = true;
    }
}

fn char_to_byte(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map_or(s.len(), |(i, _)| i)
}

#[derive(Component)]
pub struct Panel;

#[derive(Component)]
pub struct CodeView;

#[derive(Component)]
pub struct Tree;

#[derive(Component)]
pub struct FileButton(usize);

#[derive(Component)]
pub struct Header;

#[derive(Component)]
pub struct Status;

#[derive(Component)]
pub struct Toggle;

#[derive(Component)]
pub struct FixButton;

fn ui_font(fonts: &Fonts, size: f32, weight: FontWeight) -> TextFont {
    TextFont {
        font: fonts.ui.clone(),
        font_size: size,
        weight,
        ..default()
    }
}

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, fonts: Res<Fonts>) {
    // A monospace face for code.
    let mut db = fontdb::Database::new();
    for path in [
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/SFNSMono.ttf",
        "C:\\Windows\\Fonts\\consola.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            db.load_font_data(bytes);
            break;
        }
    }
    let family: Option<&'static str> =
        db.faces().next().and_then(|f| f.families.first()).map(|f| &*Box::leak(f.0.clone().into_boxed_str()));
    let mut font_system = if family.is_some() {
        FontSystem::new_with_locale_and_db("en-US".into(), db)
    } else {
        FontSystem::new()
    };
    let metrics = Metrics::new(FONT, LINE);
    let mut buffer = Buffer::new(&mut font_system, metrics);
    buffer.set_wrap(&mut font_system, Wrap::WordOrGlyph);
    let mut editor = Editor::new(buffer);
    editor.set_auto_indent(true);
    let gutter = Buffer::new(&mut font_system, metrics);
    let popup = Buffer::new(&mut font_system, metrics);
    let image = images.add(Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &BG,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    commands.insert_resource(Code {
        fonts: font_system,
        cache: SwashCache::new(),
        family,
        editor,
        gutter,
        popup,
        open: false,
        focused: false,
        swallowed: false,
        loaded: None,
        lang: Lang::C4,
        size: UVec2::ZERO,
        sf: 1.0,
        image: image.clone(),
        redraw: true,
        blink: 0.0,
        undo: Vec::new(),
        redo: Vec::new(),
        last_edit: 0.0,
        completion: None,
        dragging: false,
        last_click: (0.0, Vec2::ZERO, 0),
        note: None,
        pending_complete: false,
        pending_fix: false,
        pending_save: false,
    });

    let panel_bg = Color::srgba(0.045, 0.055, 0.075, 0.97);
    let hairline = Color::srgba(1.0, 1.0, 1.0, 0.08);
    // The button that opens the panel.
    commands.spawn((
        Button,
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(16),
            padding: UiRect::axes(px(12), px(7)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(10)),
            ..default()
        },
        BackgroundColor(panel_bg),
        BorderColor::all(hairline),
        Toggle,
        WhenLoaded,
        children![(
            Text::new("</>  Source   ⌘E"),
            ui_font(&fonts, 13.0, FontWeight::MEDIUM),
            TextColor(TEXT.with_alpha(0.85)),
        )],
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(16),
                right: px(16),
                bottom: px(52),
                width: px(PANEL_W),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(12)),
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            BackgroundColor(panel_bg),
            BorderColor::all(hairline),
            Interaction::default(),
            Panel,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    padding: UiRect::new(px(14), px(14), px(10), px(8)),
                    ..default()
                },
                Text::new("SOURCE"),
                ui_font(&fonts, 11.5, FontWeight::SEMIBOLD),
                TextColor(Color::srgba(0.72, 0.9, 1.0, 0.75)),
                Header,
            ));
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    max_height: percent(30),
                    padding: UiRect::new(px(6), px(6), px(0), px(6)),
                    border: UiRect::bottom(px(1)),
                    overflow: Overflow::scroll_y(),
                    flex_shrink: 0.0,
                    ..default()
                },
                BorderColor::all(hairline),
                ScrollPosition::default(),
                RelativeCursorPosition::default(),
                Tree,
            ));
            p.spawn((
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    ..default()
                },
                ImageNode::new(image),
                RelativeCursorPosition::default(),
                Interaction::default(),
                CodeView,
            ));
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    padding: UiRect::axes(px(14), px(7)),
                    border: UiRect::top(px(1)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BorderColor::all(hairline),
            ))
            .with_children(|row| {
                row.spawn((
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                    Text::new(""),
                    ui_font(&fonts, 12.0, FontWeight::MEDIUM),
                    TextColor(Color::srgba(0.8, 0.86, 0.95, 0.75)),
                    Status,
                ));
                row.spawn((
                    Button,
                    Node {
                        padding: UiRect::axes(px(10), px(4)),
                        border_radius: BorderRadius::all(px(6)),
                        display: Display::None,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.25, 0.5, 0.9, 0.35)),
                    FixButton,
                    children![(Text::new("Fix  ⌘."), ui_font(&fonts, 12.0, FontWeight::SEMIBOLD), TextColor(TEXT))],
                ));
            });
        });
}

/// Words that could complete `word`: keywords, element kinds and names.
fn candidates(word: &str, lang: Lang, diagram: &Diagram) -> Vec<String> {
    let model = &diagram.model;
    let lower = word.to_lowercase();
    let mut set: BTreeSet<String> = BTreeSet::new();
    let keywords = match lang {
        Lang::C4 => C4_KEYWORDS,
        Lang::Mermaid => MERMAID_KEYWORDS,
    };
    if word.contains('.') {
        for e in &model.elements {
            if !e.fqn.is_empty() {
                set.insert(e.fqn.clone());
            }
        }
    } else {
        set.extend(keywords.iter().map(|k| k.to_string()));
        for e in &model.elements {
            if let Some(short) = e.fqn.rsplit('.').next().filter(|s| !s.is_empty()) {
                set.insert(short.to_string());
            }
            if !e.kind.is_empty() {
                set.insert(e.kind.clone());
            }
        }
    }
    let mut items: Vec<String> = set
        .into_iter()
        .filter(|c| c != word && c.to_lowercase().starts_with(&lower))
        .collect();
    items.sort_by_key(|c| (!c.starts_with(word), c.len()));
    items.truncate(7);
    items
}

/// Toggle the panel, load files into the editor, and handle typing and the mouse.
#[allow(clippy::too_many_arguments)]
pub fn input(
    _main_thread: NonSendMarker,
    time: Res<Time>,
    mut code: ResMut<Code>,
    mut ws: ResMut<Workspace>,
    diagram: Res<Diagram>,
    mut watch: ResMut<Watch>,
    keys: Res<ButtonInput<KeyCode>>,
    mut events: MessageReader<KeyboardInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    view: Single<(&RelativeCursorPosition, &ComputedNode), With<CodeView>>,
    panel: Single<&Interaction, With<Panel>>,
    toggle: Query<&Interaction, (With<Toggle>, Changed<Interaction>)>,
    fix_button: Query<&Interaction, (With<FixButton>, Changed<Interaction>)>,
) {
    let code = &mut *code;
    code.swallowed = false;
    let cmd = keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let toggled = toggle.iter().any(|i| *i == Interaction::Pressed) || (cmd && keys.just_pressed(KeyCode::KeyE));
    if toggled && diagram.loaded {
        code.open = !code.open;
        code.focused = code.open;
        code.redraw = true;
    }
    if !code.open || ws.files.is_empty() {
        events.clear();
        return;
    }

    // Load the active file into the editor when it changes (or is reloaded from disk).
    let active = ws.active.min(ws.files.len() - 1);
    let key = (ws.generation(), active, ws.files[active].rev);
    if code.loaded != Some(key) {
        let same_file = code.loaded.is_some_and(|(g, i, _)| g == key.0 && i == key.1);
        code.lang = if is_mermaid(&ws.files[active].rel) { Lang::Mermaid } else { Lang::C4 };
        let text = ws.files[active].text.clone();
        let cursor = code.editor.cursor();
        code.set_text(&text);
        code.editor.set_selection(Selection::None);
        if same_file {
            let lines = code.editor.with_buffer(|b| b.lines.len());
            let line = cursor.line.min(lines.saturating_sub(1));
            let len = code.line_text(line).len();
            code.editor.set_cursor(Cursor::new(line, cursor.index.min(len)));
        } else {
            code.editor.set_cursor(Cursor::new(0, 0));
            code.editor.with_buffer_mut(|b| b.set_scroll(Default::default()));
            code.undo.clear();
            code.redo.clear();
        }
        code.completion = None;
        code.loaded = Some(key);
    }

    let now = time.elapsed_secs_f64();
    let (rel, node) = *view;
    let over_code = rel.cursor_over();
    let at = rel.normalized.map(|n| {
        let p = (n + Vec2::splat(0.5)) * node.size;
        let x0 = (GUTTER + PAD) * code.sf;
        Vec2::new(p.x - x0, p.y - PAD * code.sf)
    });

    // Mouse: focus, click (and double / triple click), drag to select, wheel to scroll.
    if mouse.just_pressed(MouseButton::Left) {
        if over_code {
            if let Some(p) = at {
                code.focused = true;
                let (t, last, n) = code.last_click;
                let n = if now - t < 0.4 && p.distance(last) < 6.0 * code.sf { n % 3 + 1 } else { 1 };
                code.last_click = (now, p, n);
                let (x, y) = (p.x as i32, p.y as i32);
                code.editor.set_selection(Selection::None);
                code.action(match n {
                    2 => Action::DoubleClick { x, y },
                    3 => Action::TripleClick { x, y },
                    _ => Action::Click { x, y },
                });
                code.dragging = n == 1;
                code.completion = None;
            }
        } else if **panel == Interaction::None {
            code.focused = false;
            code.redraw = true;
        }
    }
    if code.dragging {
        if mouse.pressed(MouseButton::Left) {
            if let Some(p) = at {
                code.action(Action::Drag { x: p.x as i32, y: p.y as i32 });
            }
        } else {
            code.dragging = false;
        }
    }
    if over_code && scroll.delta.y != 0.0 {
        let px = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y * LINE * 3.0,
            MouseScrollUnit::Pixel => scroll.delta.y,
        } * code.sf;
        code.action(Action::Scroll { pixels: -px });
    }

    let mut changed = false;
    let mut complete = std::mem::take(&mut code.pending_complete);
    let mut fix_now = fix_button.iter().any(|i| *i == Interaction::Pressed) || std::mem::take(&mut code.pending_fix);
    let events: Vec<KeyboardInput> = events.read().cloned().collect();
    if code.focused {
        for ev in events {
            if ev.state != ButtonState::Pressed {
                continue;
            }
            if cmd {
                match ev.key_code {
                    KeyCode::KeyS => save(&mut ws, &mut watch, code),
                    KeyCode::KeyZ if shift => {
                        if let Some(s) = code.redo.pop() {
                            let cur = Snapshot {
                                text: code.text(),
                                cursor: code.editor.cursor(),
                            };
                            code.undo.push(cur);
                            code.restore(s);
                            changed = true;
                        }
                    }
                    KeyCode::KeyZ => {
                        if let Some(s) = code.undo.pop() {
                            let cur = Snapshot {
                                text: code.text(),
                                cursor: code.editor.cursor(),
                            };
                            code.redo.push(cur);
                            code.restore(s);
                            changed = true;
                        }
                    }
                    KeyCode::KeyA => {
                        code.editor.set_cursor(Cursor::new(0, 0));
                        code.editor.set_selection(Selection::Normal(Cursor::new(0, 0)));
                        code.action(Action::Motion(Motion::BufferEnd));
                    }
                    KeyCode::KeyC | KeyCode::KeyX => {
                        if let Some(t) = code.editor.copy_selection() {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(t);
                            }
                            if ev.key_code == KeyCode::KeyX {
                                code.snapshot();
                                code.editor.delete_selection();
                                changed = true;
                            }
                        }
                    }
                    KeyCode::KeyV => {
                        if let Some(t) = arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()) {
                            code.snapshot();
                            code.editor.insert_string(&t, None);
                            changed = true;
                        }
                    }
                    KeyCode::Period => fix_now = true,
                    KeyCode::ArrowLeft => code.motion(Motion::Home, shift),
                    KeyCode::ArrowRight => code.motion(Motion::End, shift),
                    KeyCode::ArrowUp => code.motion(Motion::BufferStart, shift),
                    KeyCode::ArrowDown => code.motion(Motion::BufferEnd, shift),
                    KeyCode::Backspace => {
                        code.snapshot();
                        if code.editor.selection() == Selection::None {
                            code.editor.set_selection(Selection::Normal(code.editor.cursor()));
                            code.action(Action::Motion(Motion::Home));
                        }
                        code.editor.delete_selection();
                        changed = true;
                    }
                    _ => {}
                }
                continue;
            }
            let popup = code.completion.is_some();
            match ev.key_code {
                KeyCode::ArrowUp if popup => {
                    if let Some(c) = &mut code.completion {
                        c.selected = (c.selected + c.items.len() - 1) % c.items.len();
                    }
                    code.redraw = true;
                }
                KeyCode::ArrowDown if popup => {
                    if let Some(c) = &mut code.completion {
                        c.selected = (c.selected + 1) % c.items.len();
                    }
                    code.redraw = true;
                }
                KeyCode::Enter | KeyCode::Tab if popup => {
                    if let Some(c) = code.completion.take() {
                        let cur = code.editor.cursor();
                        code.snapshot();
                        code.replace(cur.line, c.start, cur.index, &c.items[c.selected]);
                        changed = true;
                    }
                }
                KeyCode::Escape if popup => {
                    code.completion = None;
                    code.redraw = true;
                }
                KeyCode::Escape => {
                    code.focused = false;
                    code.swallowed = true;
                    code.redraw = true;
                }
                KeyCode::ArrowLeft if alt => code.motion(Motion::LeftWord, shift),
                KeyCode::ArrowRight if alt => code.motion(Motion::RightWord, shift),
                KeyCode::ArrowLeft => code.motion(Motion::Left, shift),
                KeyCode::ArrowRight => code.motion(Motion::Right, shift),
                KeyCode::ArrowUp => code.motion(Motion::Up, shift),
                KeyCode::ArrowDown => code.motion(Motion::Down, shift),
                KeyCode::Home => code.motion(Motion::Home, shift),
                KeyCode::End => code.motion(Motion::End, shift),
                KeyCode::PageUp => code.motion(Motion::PageUp, shift),
                KeyCode::PageDown => code.motion(Motion::PageDown, shift),
                KeyCode::Backspace => {
                    if now - code.last_edit > 1.0 {
                        code.snapshot();
                    }
                    if alt && code.editor.selection() == Selection::None {
                        code.editor.set_selection(Selection::Normal(code.editor.cursor()));
                        code.action(Action::Motion(Motion::LeftWord));
                        code.editor.delete_selection();
                    } else {
                        // Deleting an opener next to its closer removes both.
                        let cur = code.editor.cursor();
                        let line = code.line_text(cur.line);
                        let before = line[..cur.index].chars().last();
                        let after = line[cur.index..].chars().next();
                        let pair = matches!((before, after), (Some('('), Some(')')) | (Some('['), Some(']')) | (Some('{'), Some('}')) | (Some('\''), Some('\'')) | (Some('"'), Some('"')));
                        if pair && code.editor.selection() == Selection::None {
                            code.action(Action::Delete);
                        }
                        code.action(Action::Backspace);
                    }
                    code.last_edit = now;
                    changed = true;
                    complete = code.completion.is_some();
                }
                KeyCode::Delete => {
                    code.snapshot();
                    code.action(Action::Delete);
                    changed = true;
                }
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    code.snapshot();
                    let cur = code.editor.cursor();
                    let line = code.line_text(cur.line);
                    let before = line[..cur.index].trim_end().chars().last();
                    let after = line[cur.index..].trim_start().chars().next();
                    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                    code.action(Action::Enter);
                    // After an opening brace, indent; if its closer follows, put that on its own line.
                    if matches!(before, Some('{') | Some('[') | Some('(')) {
                        code.editor.insert_string("  ", None);
                        if matches!(after, Some('}') | Some(']') | Some(')')) {
                            let keep = code.editor.cursor();
                            code.editor.insert_string(&format!("\n{indent}"), None);
                            code.editor.set_cursor(keep);
                        }
                    }
                    changed = true;
                }
                KeyCode::Tab => {
                    code.snapshot();
                    code.editor.insert_string("  ", None);
                    changed = true;
                }
                _ => {
                    let Some(text) = ev.text.as_ref().or(match &ev.logical_key {
                        Key::Character(c) => Some(c),
                        _ => None,
                    }) else {
                        continue;
                    };
                    for c in text.chars().filter(|c| !c.is_control()) {
                        if now - code.last_edit > 1.0 || c.is_whitespace() {
                            code.snapshot();
                        }
                        code.last_edit = now;
                        type_char(code, c);
                        complete = c.is_alphanumeric() || c == '_' || c == '.';
                        if !complete {
                            code.completion = None;
                        }
                    }
                    changed = true;
                }
            }
        }
    }

    if std::mem::take(&mut code.pending_save) {
        save(&mut ws, &mut watch, code);
    }

    // ⌘. or the Fix button: apply the suggested fix nearest the cursor.
    if fix_now {
        let rel = ws.files[active].rel.clone();
        let cur = code.editor.cursor();
        let line = code.line_text(cur.line);
        let col = line[..cur.index].chars().count();
        let on_line: Vec<&Diagnostic> = diagram
            .warnings
            .iter()
            .filter(|d| d.file == rel && d.line == cur.line + 1 && d.fix.is_some())
            .collect();
        let d = on_line
            .iter()
            .find(|d| d.col <= col && col <= d.col + d.len)
            .or(on_line.first())
            .copied()
            .cloned();
        if let Some(d) = d {
            let (s, e) = (char_to_byte(&line, d.col), char_to_byte(&line, d.col + d.len));
            code.snapshot();
            code.replace(cur.line, s, e, d.fix.as_deref().unwrap_or_default());
            changed = true;
            code.note = Some((format!("Fixed: {}", d.fix.unwrap_or_default()), 2.0));
        }
    }

    if changed {
        code.recolour();
        let text = code.text();
        let file = &mut ws.files[active];
        if file.text != text {
            file.text = text;
            ws.edited();
        }
    }
    if complete || (changed && code.completion.is_some()) {
        let cur = code.editor.cursor();
        let line = code.line_text(cur.line);
        let before = &line[..cur.index];
        let start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '.')
            .last()
            .map_or(cur.index, |(i, _)| i);
        let word = &before[start..];
        let items = if word.is_empty() || word.starts_with(|c: char| c.is_ascii_digit()) {
            Vec::new()
        } else {
            candidates(word, code.lang, &diagram)
        };
        code.completion = (!items.is_empty()).then_some(Completion { items, selected: 0, start });
        code.redraw = true;
    }
}

/// Drive the editor from the script runner (for checks and demos): `source`, `file NAME`,
/// `goto LINE COL`, `type TEXT` (`\n` for Enter), `fix`.
pub fn script(code: &mut Code, ws: &mut Workspace, action: &str, arg: &str) {
    match action {
        "source" => {
            code.open = true;
            code.focused = true;
        }
        "file" => {
            if let Some(i) = ws.files.iter().position(|f| f.rel == arg || f.rel.ends_with(arg)) {
                ws.active = i;
            }
        }
        "goto" => {
            let n: Vec<usize> = arg.split_whitespace().filter_map(|w| w.parse().ok()).collect();
            if let [line, col] = n[..] {
                let line = line.saturating_sub(1);
                let text = code.line_text(line);
                code.editor.set_cursor(Cursor::new(line, char_to_byte(&text, col.saturating_sub(1))));
                code.editor.set_selection(Selection::None);
                code.completion = None;
                code.redraw = true;
            }
        }
        "type" => {
            code.snapshot();
            for c in arg.replace("\\n", "\n").chars() {
                if c == '\n' {
                    code.action(Action::Enter);
                } else {
                    type_char(code, c);
                }
            }
            code.recolour();
            let text = code.text();
            let active = ws.active;
            if ws.files[active].text != text {
                ws.files[active].text = text;
                ws.edited();
            }
            // Offer completions for the word just typed.
            code.pending_complete = true;
        }
        "fix" => code.pending_fix = true,
        "save" => code.pending_save = true,
        _ => {}
    }
}

fn type_char(code: &mut Code, c: char) {
    let cur = code.editor.cursor();
    let line = code.line_text(cur.line);
    let after = line[cur.index..].chars().next();
    let before = line[..cur.index].chars().last();
    let has_selection = code.editor.selection() != Selection::None;
    // Typing a closer that is already there just steps over it.
    if !has_selection && matches!(c, ')' | ']' | '}' | '\'' | '"') && after == Some(c) {
        code.action(Action::Motion(Motion::Right));
        return;
    }
    let closer = match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '\'' | '"' if !before.is_some_and(|b| b.is_alphanumeric()) => Some(c),
        _ => None,
    };
    let next_is_open = after.is_none_or(|a| a.is_whitespace() || ")]}".contains(a));
    match closer {
        Some(close) if next_is_open && !has_selection => {
            code.editor.insert_string(&format!("{c}{close}"), None);
            code.action(Action::Motion(Motion::Left));
        }
        _ => code.action(Action::Insert(c)),
    }
}

fn save(ws: &mut Workspace, watch: &mut Watch, code: &mut Code) {
    let mut saved = 0;
    let mut failed = None;
    for f in ws.files.iter_mut().filter(|f| f.dirty()) {
        let Some(path) = &f.path else { continue };
        match std::fs::write(path, &f.text) {
            Ok(()) => {
                f.saved = f.text.clone();
                saved += 1;
            }
            Err(e) => failed = Some(format!("couldn't save {}: {e}", f.rel)),
        }
    }
    // Don't let the file watcher treat our own save as an outside change.
    watch.stamp = ws.files.iter().filter_map(|f| f.path.as_ref()).filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()).max();
    let msg = match (failed, saved, ws.files.iter().any(|f| f.path.is_none())) {
        (Some(e), _, _) => e,
        (None, 0, true) => "The built-in example isn't on disk; open a folder to save changes".into(),
        (None, 0, false) => "Nothing to save".into(),
        (None, n, _) => format!("Saved {n} file{}", if n == 1 { "" } else { "s" }),
    };
    code.note = Some((msg, 2.5));
}

/// Size the image to the panel, draw the editor into it, and keep the tree, header and
/// status line current.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    time: Res<Time>,
    mut code: ResMut<Code>,
    mut ws: ResMut<Workspace>,
    diagram: Res<Diagram>,
    fonts: Res<Fonts>,
    mut images: ResMut<Assets<Image>>,
    window: Single<&Window>,
    mut panel: Single<&mut Node, With<Panel>>,
    view: Single<&ComputedNode, With<CodeView>>,
    tree: Single<(Entity, &RelativeCursorPosition, &mut ScrollPosition), With<Tree>>,
    (mut header, mut status): (
        Single<&mut Text, (With<Header>, Without<Status>)>,
        Single<&mut Text, (With<Status>, Without<Header>)>,
    ),
    (mut fix_btn, mut views_panel): (
        Single<&mut Node, (With<FixButton>, Without<Panel>, Without<crate::story::ViewsPanel>)>,
        Query<&mut Node, (With<crate::story::ViewsPanel>, Without<Panel>, Without<FixButton>)>,
    ),
    buttons: Query<(&Interaction, &FileButton), Changed<Interaction>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut built: Local<Option<(u64, usize, Vec<bool>)>>,
) {
    let code = &mut *code;
    let display = if code.open && diagram.loaded { Display::Flex } else { Display::None };
    if panel.display != display {
        panel.display = display;
    }
    for mut n in &mut views_panel {
        n.right = px(if code.open { 16.0 + PANEL_W + 12.0 } else { 16.0 });
    }
    if display == Display::None || ws.files.is_empty() {
        return;
    }
    let active = ws.active.min(ws.files.len() - 1);

    // File tree: folders, then their files; click a file to open it.
    for (i, b) in &buttons {
        if *i == Interaction::Pressed {
            ws.active = b.0;
            code.focused = true;
        }
    }
    let (tree_e, tree_rel, mut tree_scroll) = tree.into_inner();
    if tree_rel.cursor_over() && scroll.delta.y != 0.0 {
        tree_scroll.y = (tree_scroll.y - scroll.delta.y * 20.0).max(0.0);
    }
    let dirty: Vec<bool> = ws.files.iter().map(|f| f.dirty()).collect();
    let state = (ws.generation(), ws.active, dirty.clone());
    if built.as_ref() != Some(&state) {
        *built = Some(state);
        commands.entity(tree_e).despawn_related::<Children>();
        let mut order: Vec<usize> = (0..ws.files.len()).collect();
        order.sort_by(|a, b| ws.files[*a].rel.cmp(&ws.files[*b].rel));
        let mut shown_dirs: BTreeSet<String> = BTreeSet::new();
        for i in order {
            let rel = &ws.files[i].rel;
            let parts: Vec<&str> = rel.split('/').collect();
            for d in 1..parts.len() {
                let dir = parts[..d].join("/");
                if shown_dirs.insert(dir) {
                    commands.spawn((
                        Node {
                            padding: UiRect::new(px(8.0 + 14.0 * (d - 1) as f32), px(8), px(3), px(3)),
                            ..default()
                        },
                        Text::new(format!("{}/", parts[d - 1])),
                        ui_font(&fonts, 12.5, FontWeight::SEMIBOLD),
                        TextColor(Color::srgba(0.72, 0.8, 0.92, 0.6)),
                        ChildOf(tree_e),
                    ));
                }
            }
            let on = i == active;
            let name = format!("{}{}", parts[parts.len() - 1], if dirty[i] { "  •" } else { "" });
            commands.spawn((
                Button,
                Node {
                    padding: UiRect::new(px(8.0 + 14.0 * (parts.len() - 1) as f32), px(8), px(4), px(4)),
                    border_radius: BorderRadius::all(px(6)),
                    ..default()
                },
                BackgroundColor(if on { Color::srgba(0.3, 0.55, 0.95, 0.28) } else { Color::NONE }),
                FileButton(i),
                ChildOf(tree_e),
                children![(
                    Text::new(name),
                    ui_font(&fonts, 12.5, if on { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }),
                    TextColor(if on { TEXT } else { Color::srgba(0.85, 0.9, 1.0, 0.75) }),
                )],
            ));
        }
    }

    let file = &ws.files[active];
    let kind = if is_mermaid(&file.rel) { "MERMAID" } else { "LIKEC4" };
    let head = format!("SOURCE  ·  {kind}  ·  {}{}", file.rel, if file.dirty() { "  (unsaved)" } else { "" });
    if header.0 != head {
        header.0 = head;
    }

    // Status: the problem under the cursor (with its fix), else position and counts.
    let diags: Vec<&Diagnostic> = diagram.warnings.iter().filter(|d| d.file == file.rel).collect();
    let cur = code.editor.cursor();
    let line = code.line_text(cur.line);
    let col = line[..cur.index.min(line.len())].chars().count();
    let here = diags
        .iter()
        .filter(|d| d.line == cur.line + 1)
        .min_by_key(|d| (d.col > col || col > d.col + d.len) as u8);
    if let Some((_, t)) = &mut code.note {
        *t -= time.delta_secs();
    }
    if code.note.as_ref().is_some_and(|(_, t)| *t <= 0.0) {
        code.note = None;
    }
    let text = match (&code.note, here) {
        (Some((n, _)), _) => n.clone(),
        (None, Some(d)) => format!("{}  {}", if d.error { "Error:" } else { "Warning:" }, d.message),
        (None, None) => {
            let total = diagram.warnings.len();
            let problems = match total {
                0 => "no problems".to_string(),
                1 => "1 problem".to_string(),
                n => format!("{n} problems"),
            };
            format!("Ln {}, Col {}   ·   {problems}   ·   ⌘S save", cur.line + 1, col + 1)
        }
    };
    if status.0 != text {
        status.0 = text;
    }
    let show_fix = code.note.is_none() && here.is_some_and(|d| d.fix.is_some());
    let fix_display = if show_fix { Display::Flex } else { Display::None };
    if fix_btn.display != fix_display {
        fix_btn.display = fix_display;
    }

    // Size the image to the view, in physical pixels.
    let sf = window.scale_factor();
    let size = view.size.as_uvec2();
    if size.x < 8 || size.y < 8 {
        return;
    }
    if size != code.size || sf != code.sf {
        code.size = size;
        code.sf = sf;
        let metrics = Metrics::new(FONT * sf, LINE * sf);
        let text_w = size.x as f32 - (GUTTER + 2.0 * PAD) * sf;
        let text_h = size.y as f32 - 2.0 * PAD * sf;
        let fonts_sys = &mut code.fonts;
        code.editor.with_buffer_mut(|b| {
            b.set_metrics_and_size(fonts_sys, metrics, Some(text_w), Some(text_h));
        });
        code.gutter.set_metrics_and_size(fonts_sys, metrics, Some((GUTTER - 10.0) * sf), Some(text_h + LINE * sf));
        code.popup.set_metrics(fonts_sys, metrics);
        if let Some(img) = images.get_mut(&code.image) {
            *img = Image::new_fill(
                Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &BG,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
        }
        code.redraw = true;
    }

    // Blink the caret.
    let before = code.blink < 0.55;
    code.blink = (code.blink + time.delta_secs()) % 1.1;
    if code.focused && before != (code.blink < 0.55) {
        code.redraw = true;
    }
    if diagram.is_changed() {
        code.redraw = true;
    }
    if !code.redraw {
        return;
    }
    code.redraw = false;
    let pixels = render(code, &diags);
    if let Some(img) = images.get_mut(&code.image) {
        img.data = Some(pixels);
    }
}

fn render(code: &mut Code, diags: &[&Diagnostic]) -> Vec<u8> {
    let (w, h) = (code.size.x as i32, code.size.y as i32);
    let sf = code.sf;
    let mut px = vec![0u8; (w * h * 4) as usize];
    for p in px.chunks_exact_mut(4) {
        p.copy_from_slice(&BG);
    }
    let blend = |px: &mut [u8], x: i32, y: i32, rw: i32, rh: i32, c: [u8; 4], x_min: i32| {
        let a = c[3] as f32 / 255.0;
        for yy in y.max(0)..(y + rh).min(h) {
            for xx in x.max(x_min)..(x + rw).min(w) {
                let i = ((yy * w + xx) * 4) as usize;
                for k in 0..3 {
                    px[i + k] = (px[i + k] as f32 * (1.0 - a) + c[k] as f32 * a) as u8;
                }
            }
        }
    };
    let pad = (PAD * sf) as i32;
    let x0 = ((GUTTER + PAD) * sf) as i32;
    let line_h = LINE * sf;

    {
        let fonts = &mut code.fonts;
        code.editor.shape_as_needed(fonts, true);
    }

    // Current line.
    if code.focused {
        if let Some((_, cy)) = code.editor.cursor_position() {
            blend(&mut px, 0, cy + pad, w, line_h as i32, LINE_BG, 0);
        }
    }

    // Line numbers, beside the first visual line of each source line.
    let runs: Vec<(usize, f32, bool)> = code.editor.with_buffer(|b| {
        let mut out = Vec::new();
        let mut last = usize::MAX;
        for run in b.layout_runs() {
            out.push((run.line_i, run.line_top, run.line_i != last));
            last = run.line_i;
        }
        out
    });
    if let Some(&(_, top, _)) = runs.first() {
        let numbers: Vec<String> = runs.iter().map(|(l, _, first)| if *first { (l + 1).to_string() } else { String::new() }).collect();
        let attrs = match code.family {
            Some(f) => Attrs::new().family(Family::Name(f)),
            None => Attrs::new().family(Family::Monospace),
        };
        let fonts = &mut code.fonts;
        code.gutter.set_text(fonts, &numbers.join("\n"), &attrs, Shaping::Advanced, Some(Align::Right));
        code.gutter.shape_until_scroll(fonts, false);
        let gy = pad + top as i32;
        code.gutter.draw(fonts, &mut code.cache, GUTTER_INK, |x, y, gw, gh, c| {
            blend(&mut px, x, y + gy, gw as i32, gh as i32, [c.r(), c.g(), c.b(), c.a()], 0);
        });
    }

    // Selection, text and caret.
    let caret = if code.focused && code.blink < 0.55 {
        Ink::rgba(CARET[0], CARET[1], CARET[2], 255)
    } else {
        Ink::rgba(0, 0, 0, 0)
    };
    let fonts = &mut code.fonts;
    code.editor.draw(
        fonts,
        &mut code.cache,
        ink(Token::Plain),
        caret,
        Ink::rgba(SELECTION[0], SELECTION[1], SELECTION[2], SELECTION[3]),
        ink(Token::Plain),
        |x, y, gw, gh, c| {
            blend(&mut px, x + x0, y + pad, gw as i32, gh as i32, [c.r(), c.g(), c.b(), c.a()], x0 - 2);
        },
    );

    // Squiggles under problems.
    let waves: Vec<(i32, i32, i32, [u8; 4])> = code.editor.with_buffer(|b| {
        let mut out = Vec::new();
        for run in b.layout_runs() {
            let text = b.lines[run.line_i].text();
            for d in diags.iter().filter(|d| d.line == run.line_i + 1) {
                let (mut s, mut e) = (char_to_byte(text, d.col), char_to_byte(text, d.col + d.len.max(1)));
                if d.len == 0 {
                    s = text.len() - text.trim_start().len();
                    e = text.len();
                }
                let xs: Vec<(f32, f32)> = run.glyphs.iter().filter(|g| g.start < e && g.end > s).map(|g| (g.x, g.x + g.w)).collect();
                if let (Some(a), Some(z)) = (xs.iter().map(|p| p.0).reduce(f32::min), xs.iter().map(|p| p.1).reduce(f32::max)) {
                    let y = run.line_top + line_h - 3.0 * sf;
                    out.push((a as i32, z as i32, y as i32, if d.error { ERROR } else { WARNING }));
                }
            }
        }
        out
    });
    for (a, z, y, c) in waves {
        for x in a..z {
            let wy = (x as f32 / (2.2 * sf)).sin() * 1.6 * sf;
            blend(&mut px, x + x0, y + pad + wy as i32, 1, (1.4 * sf).ceil() as i32, c, x0 - 2);
        }
    }

    // Autocomplete popup, below the caret.
    if let (Some(c), Some((cx, cy))) = (&code.completion, code.editor.cursor_position()) {
        let attrs = match code.family {
            Some(f) => Attrs::new().family(Family::Name(f)),
            None => Attrs::new().family(Family::Monospace),
        };
        let fonts = &mut code.fonts;
        code.popup.set_size(fonts, None, None);
        code.popup.set_text(fonts, &c.items.join("\n"), &attrs, Shaping::Advanced, None);
        code.popup.shape_until_scroll(fonts, false);
        let text_w = code.popup.layout_runs().map(|r| r.line_w).fold(0.0, f32::max);
        let bw = (text_w + 20.0 * sf) as i32;
        let bh = (c.items.len() as f32 * line_h + 8.0 * sf) as i32;
        let mut bx = cx + x0 - (6.0 * sf) as i32;
        let mut by = cy + pad + line_h as i32 + (2.0 * sf) as i32;
        bx = bx.min(w - bw - 4).max(0);
        if by + bh > h {
            by = cy + pad - bh - (2.0 * sf) as i32;
        }
        blend(&mut px, bx - 1, by - 1, bw + 2, bh + 2, [90, 120, 170, 255], 0);
        blend(&mut px, bx, by, bw, bh, [24, 29, 40, 255], 0);
        let sel_y = by + (4.0 * sf) as i32 + (c.selected as f32 * line_h) as i32;
        blend(&mut px, bx, sel_y, bw, line_h as i32, [60, 100, 170, 200], 0);
        let (tx, ty) = (bx + (10.0 * sf) as i32, by + (4.0 * sf) as i32);
        code.popup.draw(fonts, &mut code.cache, ink(Token::Plain), |x, y, gw, gh, col| {
            blend(&mut px, x + tx, y + ty, gw as i32, gh as i32, [col.r(), col.g(), col.b(), col.a()], 0);
        });
    }
    px
}
