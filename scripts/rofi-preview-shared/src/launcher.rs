//! Shared GTK launcher used by the clipboard and file-search applications.
//!
//! Selection, filtering, mode changes and preview notifications live in this
//! process. An external helper cannot reliably observe or alter those pieces
//! of Rofi's internal state, which is why this module replaces the old patch.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Entry,
    EventControllerKey, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    PropagationPhase, ScrolledWindow, SelectionMode, gdk, gio, glib,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config;

const THEME_RELOAD_INTERVAL: Duration = Duration::from_millis(250);

pub type UiResult<T> = Result<T, String>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mode {
    pub id: String,
    pub label: String,
}

impl Mode {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Icon {
    Name(String),
    File(PathBuf),
    Image(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub search_text: String,
    pub icon: Option<Icon>,
    pub active: bool,
    pub permanent: bool,
}

impl Row {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            id: id.into(),
            search_text: title.clone(),
            title,
            subtitle: None,
            icon: None,
            active: false,
            permanent: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub shortcut: Option<char>,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Controls {
    #[default]
    ModesTop,
    ModesBottom,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    pub controls: Controls,
    pub show_prompt: bool,
    pub search_placeholder: String,
    pub mode_buttons_expand: bool,
    pub action_buttons_expand: bool,
    pub show_icons: bool,
    pub icon_size: i32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            controls: Controls::ModesTop,
            show_prompt: true,
            search_placeholder: String::new(),
            mode_buttons_expand: true,
            action_buttons_expand: true,
            show_icons: true,
            icon_size: 32,
        }
    }
}

impl Action {
    pub fn new(id: impl Into<String>, label: impl Into<String>, shortcut: Option<char>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            shortcut,
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct View {
    pub prompt: String,
    pub rows: Vec<Row>,
    pub actions: Vec<Action>,
    pub selected: Option<String>,
    pub empty_message: Option<String>,
    pub layout: Layout,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Outcome {
    #[default]
    None,
    Refresh {
        selected: Option<String>,
    },
    Close,
}

pub trait Controller {
    fn application_id(&self) -> &'static str;
    fn namespace(&self) -> &'static str;
    fn modes(&self) -> Vec<Mode>;
    fn active_mode(&self) -> &str;
    fn switch_mode(&mut self, mode: &str) -> UiResult<View>;
    fn view(&mut self) -> UiResult<View>;
    fn activate(&mut self, row: &str) -> UiResult<Outcome>;
    fn action(&mut self, action: &str, selected: Option<&str>) -> UiResult<Outcome>;
    fn selection_changed(&mut self, selected: &str, serial: u64) -> UiResult<()>;
    fn close(&mut self) -> UiResult<()> {
        Ok(())
    }
}

struct State {
    controller: Box<dyn Controller>,
    window: ApplicationWindow,
    backdrop: ApplicationWindow,
    root: GtkBox,
    mode_bar: GtkBox,
    prompt: Label,
    search: Entry,
    list: ListBox,
    action_bar: GtkBox,
    bottom_bar: GtkBox,
    status: Label,
    rows: Vec<Row>,
    visible_ids: Vec<String>,
    actions: Vec<Action>,
    empty_message: String,
    rendering: bool,
    selection_serial: u64,
    layout: Layout,
}

pub fn run(controller: impl Controller + 'static) {
    let application_id = controller.application_id();
    let pending: Rc<RefCell<Option<Box<dyn Controller>>>> =
        Rc::new(RefCell::new(Some(Box::new(controller))));
    let application = Application::builder()
        .application_id(application_id)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.connect_activate(move |application| {
        let Some(controller) = pending.borrow_mut().take() else {
            return;
        };
        build_window(application, controller);
    });
    application.run_with_args(&[application_id]);
}

fn build_window(application: &Application, controller: Box<dyn Controller>) {
    let geometry = launcher_geometry();
    let namespace = controller.namespace();
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_icon_theme_name(Some("WhiteSur-dark"));
    }
    let backdrop = ApplicationWindow::builder()
        .application(application)
        .decorated(false)
        .build();
    backdrop.add_css_class("rofi-preview-shared-backdrop");
    backdrop.init_layer_shell();
    backdrop.set_layer(Layer::Overlay);
    backdrop.set_namespace(Some("rofi-preview-shared-backdrop"));
    backdrop.set_keyboard_mode(KeyboardMode::None);
    for edge in [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
        backdrop.set_anchor(edge, true);
    }
    let window = ApplicationWindow::builder()
        .application(application)
        .default_height(geometry.height)
        .default_width(geometry.companion_width)
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("rofi-preview-shared-window");
    window.add_css_class(namespace);
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some(namespace));
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_margin(Edge::Top, 44);
    window.set_margin(Edge::Right, 5);

    let root = GtkBox::new(Orientation::Vertical, 10);
    root.add_css_class("rofi-preview-shared-root");
    let mode_bar = GtkBox::new(Orientation::Horizontal, 10);
    mode_bar.add_css_class("rofi-preview-shared-modes");

    let input = GtkBox::new(Orientation::Horizontal, 8);
    input.add_css_class("rofi-preview-shared-input");
    let prompt = Label::new(None);
    prompt.add_css_class("rofi-preview-shared-prompt");
    let search = Entry::new();
    search.set_hexpand(true);
    search.set_placeholder_text(Some("Search"));
    search.add_css_class("rofi-preview-shared-search");
    input.append(&prompt);
    input.append(&search);

    let list = ListBox::new();
    list.set_activate_on_single_click(true);
    list.set_selection_mode(SelectionMode::Single);
    list.add_css_class("rofi-preview-shared-list");
    let scroller = ScrolledWindow::new();
    scroller.set_child(Some(&list));
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_policy(PolicyType::Never, PolicyType::Automatic);

    let action_bar = GtkBox::new(Orientation::Horizontal, 10);
    action_bar.set_halign(Align::Fill);
    action_bar.add_css_class("rofi-preview-shared-actions");
    let bottom_bar = GtkBox::new(Orientation::Horizontal, 10);
    bottom_bar.set_halign(Align::Fill);
    bottom_bar.add_css_class("rofi-preview-shared-bottom");
    bottom_bar.append(&action_bar);
    let status = Label::new(None);
    status.set_halign(Align::Start);
    status.set_wrap(true);
    status.set_visible(false);
    status.add_css_class("rofi-preview-shared-error");

    root.append(&mode_bar);
    root.append(&input);
    root.append(&scroller);
    root.append(&bottom_bar);
    root.append(&status);
    window.set_child(Some(&root));

    let state = Rc::new(RefCell::new(State {
        controller,
        window: window.clone(),
        backdrop: backdrop.clone(),
        root,
        mode_bar,
        prompt,
        search: search.clone(),
        list: list.clone(),
        action_bar,
        bottom_bar,
        status,
        rows: Vec::new(),
        visible_ids: Vec::new(),
        actions: Vec::new(),
        empty_message: "Nothing here yet".to_owned(),
        rendering: false,
        selection_serial: 0,
        layout: Layout::default(),
    }));

    rebuild_modes(&state);
    connect_signals(&state);
    install_theme();
    let initial_view = { state.borrow_mut().controller.view() };
    match initial_view {
        Ok(view) => render_view(&state, view, None),
        Err(error) => show_error(&state, error),
    }

    let state_for_close = Rc::clone(&state);
    window.connect_close_request(move |_| {
        if let Ok(mut state) = state_for_close.try_borrow_mut()
            && let Err(error) = state.controller.close()
        {
            eprintln!("rofi-preview-shared: close controller: {error}");
        }
        state_for_close.borrow().backdrop.close();
        glib::Propagation::Proceed
    });

    backdrop.present();
    window.present();
    search.grab_focus();
}

fn connect_signals(state: &Rc<RefCell<State>>) {
    let outside_click = gtk::GestureClick::new();
    let clicked = Rc::clone(state);
    outside_click.connect_pressed(move |_, _, _, _| close_window(&clicked));
    state.borrow().backdrop.add_controller(outside_click);

    let changed = Rc::clone(state);
    state.borrow().search.connect_changed(move |_| {
        let rendering = changed.borrow().rendering;
        if !rendering {
            apply_filter(&changed, None);
        }
    });

    let selection = Rc::clone(state);
    state
        .borrow()
        .list
        .connect_selected_rows_changed(move |_| notify_selection(&selection));

    let activated = Rc::clone(state);
    state.borrow().list.connect_row_activated(move |_, row| {
        let id = activated
            .borrow()
            .visible_ids
            .get(row.index().max(0) as usize)
            .cloned();
        if let Some(id) = id {
            invoke(&activated, |controller| controller.activate(&id));
        }
    });

    let keys = EventControllerKey::new();
    keys.set_propagation_phase(PropagationPhase::Capture);
    let keyed = Rc::clone(state);
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        if key == gdk::Key::Escape || (control && key == gdk::Key::g) {
            close_window(&keyed);
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Down || (control && key == gdk::Key::n) {
            move_selection(&keyed, 1);
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Up
            || (!control && key == gdk::Key::ISO_Left_Tab)
            || (control && key == gdk::Key::p)
        {
            move_selection(&keyed, -1);
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Page_Down {
            move_selection(&keyed, 5);
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Page_Up {
            move_selection(&keyed, -5);
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Home || (control && key == gdk::Key::a) {
            move_selection_to(&keyed, 0);
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::End || (control && key == gdk::Key::e) {
            move_selection_to(&keyed, i32::MAX);
            return glib::Propagation::Stop;
        }
        if (control && shift && matches!(key, gdk::Key::Tab | gdk::Key::ISO_Left_Tab))
            || (shift && key == gdk::Key::Left)
        {
            cycle_mode(&keyed, -1);
            return glib::Propagation::Stop;
        }
        if (control && !shift && key == gdk::Key::Tab) || (shift && key == gdk::Key::Right) {
            cycle_mode(&keyed, 1);
            return glib::Propagation::Stop;
        }
        if shift && key == gdk::Key::Delete {
            invoke_named_action(&keyed, "delete");
            return glib::Propagation::Stop;
        }
        if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
            || (control && matches!(key, gdk::Key::j | gdk::Key::m))
        {
            activate_selected(&keyed);
            return glib::Propagation::Stop;
        }
        if modifiers.contains(gdk::ModifierType::ALT_MASK)
            && let Some(character) = key.to_unicode().map(|value| value.to_ascii_lowercase())
        {
            let action = keyed
                .borrow()
                .actions
                .iter()
                .find(|action| action.enabled && action.shortcut == Some(character))
                .map(|action| action.id.clone());
            if let Some(action) = action {
                invoke_action(&keyed, &action);
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
    state.borrow().window.add_controller(keys);
}

fn rebuild_modes(state: &Rc<RefCell<State>>) {
    let (modes, active, bar, expand) = {
        let state = state.borrow();
        (
            state.controller.modes(),
            state.controller.active_mode().to_owned(),
            state.mode_bar.clone(),
            state.layout.mode_buttons_expand,
        )
    };
    clear_box(&bar);
    for mode in modes {
        let button = Button::with_label(&mode.label);
        button.set_hexpand(expand);
        button.add_css_class("rofi-preview-shared-mode");
        if mode.id == active {
            button.add_css_class("active");
        }
        let state = Rc::clone(state);
        button.connect_clicked(move |_| switch_mode(&state, &mode.id));
        bar.append(&button);
    }
}

fn switch_mode(state: &Rc<RefCell<State>>, mode: &str) {
    let result = state.borrow_mut().controller.switch_mode(mode);
    match result {
        Ok(view) => {
            let list = {
                let mut state = state.borrow_mut();
                state.rendering = true;
                state.list.clone()
            };
            list.unselect_all();
            state.borrow_mut().rendering = false;
            render_view(state, view, None);
        }
        Err(error) => show_error(state, error),
    }
}

fn render_view(state: &Rc<RefCell<State>>, view: View, requested: Option<String>) {
    let selected = requested.or(view.selected.clone());
    {
        let mut state = state.borrow_mut();
        state.prompt.set_text(&view.prompt);
        state.prompt.set_visible(view.layout.show_prompt);
        state
            .search
            .set_placeholder_text(Some(&view.layout.search_placeholder));
        state.rows = view.rows;
        state.actions = view.actions;
        state.empty_message = view
            .empty_message
            .unwrap_or_else(|| "Nothing here yet".to_owned());
        state.layout = view.layout;
        state.status.set_visible(false);
    }
    arrange_controls(state);
    rebuild_modes(state);
    rebuild_actions(state);
    apply_filter(state, selected);
}

fn arrange_controls(state: &Rc<RefCell<State>>) {
    let (root, mode_bar, bottom_bar, controls) = {
        let state = state.borrow();
        (
            state.root.clone(),
            state.mode_bar.clone(),
            state.bottom_bar.clone(),
            state.layout.controls,
        )
    };
    if let Some(parent) = mode_bar.parent()
        && let Ok(parent) = parent.downcast::<GtkBox>()
    {
        parent.remove(&mode_bar);
    }
    match controls {
        Controls::ModesTop => root.prepend(&mode_bar),
        Controls::ModesBottom => bottom_bar.prepend(&mode_bar),
    }
}

fn rebuild_actions(state: &Rc<RefCell<State>>) {
    let (bar, bottom_bar, actions, expand) = {
        let state = state.borrow();
        (
            state.action_bar.clone(),
            state.bottom_bar.clone(),
            state.actions.clone(),
            state.layout.action_buttons_expand,
        )
    };
    clear_box(&bar);
    for action in actions {
        let button = Button::with_label(&action.label);
        button.set_hexpand(expand);
        button.set_sensitive(action.enabled);
        button.add_css_class("rofi-preview-shared-action");
        let state = Rc::clone(state);
        button.connect_clicked(move |_| invoke_action(&state, &action.id));
        bar.append(&button);
    }
    bar.set_visible(bar.first_child().is_some());
    bottom_bar.set_visible(bottom_bar.first_child().is_some());
}

fn invoke_action(state: &Rc<RefCell<State>>, action: &str) {
    let selected = selected_id(&state.borrow());
    invoke(state, |controller| {
        controller.action(action, selected.as_deref())
    });
}

fn invoke(state: &Rc<RefCell<State>>, call: impl FnOnce(&mut dyn Controller) -> UiResult<Outcome>) {
    let result = {
        let mut state = state.borrow_mut();
        call(state.controller.as_mut())
    };
    match result {
        Ok(Outcome::None) => {}
        Ok(Outcome::Close) => close_window(state),
        Ok(Outcome::Refresh { selected }) => {
            let view = state.borrow_mut().controller.view();
            match view {
                Ok(view) => render_view(state, view, selected),
                Err(error) => show_error(state, error),
            }
        }
        Err(error) => show_error(state, error),
    }
}

fn apply_filter(state: &Rc<RefCell<State>>, requested: Option<String>) {
    let (previously_selected, previous_index) = {
        let state = state.borrow();
        (
            requested.or_else(|| selected_id(&state)),
            state
                .list
                .selected_row()
                .map(|row| row.index().max(0) as usize),
        )
    };
    let (list, rows, query, empty_message, show_icons, icon_size) = {
        let state = state.borrow();
        (
            state.list.clone(),
            state.rows.clone(),
            state.search.text().to_string(),
            state.empty_message.clone(),
            state.layout.show_icons,
            state.layout.icon_size,
        )
    };
    let visible = visible_rows(&rows, &query);

    {
        let mut state = state.borrow_mut();
        state.rendering = true;
        state.visible_ids.clear();
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        for index in visible {
            let row = &rows[index];
            state.visible_ids.push(row.id.clone());
            list.append(&build_row(row, show_icons, icon_size));
        }
        if state.visible_ids.is_empty() {
            list.append(&build_empty_row(if query.trim().is_empty() {
                &empty_message
            } else {
                "No matches"
            }));
        } else {
            let selected_index = previously_selected
                .as_ref()
                .and_then(|id| state.visible_ids.iter().position(|visible| visible == id))
                .unwrap_or_else(|| {
                    previous_index
                        .unwrap_or(0)
                        .min(state.visible_ids.len().saturating_sub(1))
                });
            if let Some(row) = list.row_at_index(selected_index as i32) {
                list.select_row(Some(&row));
            }
        }
        state.rendering = false;
    }
    notify_selection(state);
}

fn visible_rows(rows: &[Row], query: &str) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.permanent || token_matches(&row.search_text, query)).then_some(index)
        })
        .collect()
}

fn build_row(row: &Row, show_icons: bool, icon_size: i32) -> ListBoxRow {
    let container = GtkBox::new(Orientation::Horizontal, 10);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(6);
    container.set_margin_bottom(6);
    if show_icons
        && let Some(icon) = row
            .icon
            .as_ref()
            .and_then(|icon| icon_widget(icon, icon_size))
    {
        container.append(&icon);
    }
    let labels = GtkBox::new(Orientation::Vertical, 1);
    labels.set_hexpand(true);
    let title = Label::new(Some(&row.title));
    title.set_halign(Align::Start);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("rofi-preview-shared-title");
    labels.append(&title);
    if let Some(subtitle) = row.subtitle.as_deref() {
        let subtitle = Label::new(Some(subtitle));
        subtitle.set_halign(Align::Start);
        subtitle.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        subtitle.add_css_class("rofi-preview-shared-subtitle");
        labels.append(&subtitle);
    }
    container.append(&labels);
    let list_row = ListBoxRow::new();
    list_row.set_child(Some(&container));
    list_row.add_css_class("rofi-preview-shared-row");
    if row.active {
        list_row.add_css_class("active");
    }
    list_row
}

fn build_empty_row(message: &str) -> ListBoxRow {
    let label = Label::new(Some(message));
    label.set_margin_top(24);
    label.set_margin_bottom(24);
    label.add_css_class("rofi-preview-shared-empty");
    let row = ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_child(Some(&label));
    row
}

fn icon_widget(icon: &Icon, icon_size: i32) -> Option<Image> {
    let image = match icon {
        Icon::Name(name) => {
            let path = Path::new(name);
            if path.is_absolute() && path.exists() {
                Image::from_file(path)
            } else {
                let display = gdk::Display::default()?;
                let theme = gtk::IconTheme::for_display(&display);
                let name = name
                    .split(',')
                    .map(str::trim)
                    .find(|candidate| !candidate.is_empty() && theme.has_icon(candidate))
                    .unwrap_or(name);
                Image::from_icon_name(name)
            }
        }
        Icon::Image(path) => Image::from_file(path),
        Icon::File(path) => {
            let file = gio::File::for_path(path);
            let info = file
                .query_info(
                    "standard::icon,standard::content-type,thumbnail::path,thumbnail::is-valid",
                    gio::FileQueryInfoFlags::NONE,
                    None::<&gio::Cancellable>,
                )
                .ok()?;
            if info.boolean("thumbnail::is-valid")
                && let Some(thumbnail) = info.attribute_string("thumbnail::path")
            {
                Image::from_file(Path::new(thumbnail.as_str()))
            } else if info
                .content_type()
                .is_some_and(|content_type| content_type.starts_with("image/"))
            {
                Image::from_file(path)
            } else {
                info.icon().map(|icon| Image::from_gicon(&icon))?
            }
        }
    };
    image.set_pixel_size(icon_size);
    image.add_css_class("rofi-preview-shared-icon");
    Some(image)
}

fn notify_selection(state: &Rc<RefCell<State>>) {
    let mut state = match state.try_borrow_mut() {
        Ok(state) if !state.rendering => state,
        _ => return,
    };
    let Some(id) = selected_id(&state) else {
        return;
    };
    state.selection_serial = state.selection_serial.saturating_add(1);
    let serial = state.selection_serial;
    if let Err(error) = state.controller.selection_changed(&id, serial) {
        state.status.set_text(&error);
        state.status.set_visible(true);
    }
}

fn selected_id(state: &State) -> Option<String> {
    let index = state.list.selected_row()?.index();
    state.visible_ids.get(index.max(0) as usize).cloned()
}

fn move_selection(state: &Rc<RefCell<State>>, delta: i32) {
    let (list, search, visible_len) = {
        let state = state.borrow();
        (
            state.list.clone(),
            state.search.clone(),
            state.visible_ids.len(),
        )
    };
    if visible_len == 0 {
        return;
    }
    let current = list.selected_row().map(|row| row.index()).unwrap_or(0);
    let target = (current + delta).clamp(0, visible_len as i32 - 1);
    if let Some(row) = list.row_at_index(target) {
        list.select_row(Some(&row));
        row.grab_focus();
        search.grab_focus();
    }
}

fn move_selection_to(state: &Rc<RefCell<State>>, target: i32) {
    let (list, search, visible_len) = {
        let state = state.borrow();
        (
            state.list.clone(),
            state.search.clone(),
            state.visible_ids.len(),
        )
    };
    if visible_len == 0 {
        return;
    }
    let target = target.clamp(0, visible_len as i32 - 1);
    if let Some(row) = list.row_at_index(target) {
        list.select_row(Some(&row));
        row.grab_focus();
        search.grab_focus();
    }
}

fn activate_selected(state: &Rc<RefCell<State>>) {
    let Some(selected) = selected_id(&state.borrow()) else {
        return;
    };
    invoke(state, |controller| controller.activate(&selected));
}

fn invoke_named_action(state: &Rc<RefCell<State>>, id: &str) -> bool {
    let enabled = state
        .borrow()
        .actions
        .iter()
        .any(|action| action.id == id && action.enabled);
    if enabled {
        invoke_action(state, id);
    }
    enabled
}

fn cycle_mode(state: &Rc<RefCell<State>>, delta: isize) {
    let (modes, active) = {
        let state = state.borrow();
        (
            state.controller.modes(),
            state.controller.active_mode().to_owned(),
        )
    };
    if modes.is_empty() {
        return;
    }
    let current = modes.iter().position(|mode| mode.id == active).unwrap_or(0) as isize;
    let target = (current + delta).rem_euclid(modes.len() as isize) as usize;
    switch_mode(state, &modes[target].id);
}

fn close_window(state: &Rc<RefCell<State>>) {
    let (window, backdrop) = {
        let state = state.borrow();
        (state.window.clone(), state.backdrop.clone())
    };
    backdrop.close();
    window.close();
}

fn show_error(state: &Rc<RefCell<State>>, error: String) {
    let state = state.borrow();
    state.status.set_text(&error);
    state.status.set_visible(true);
}

fn clear_box(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn token_matches(value: &str, query: &str) -> bool {
    let value = value.to_lowercase();
    query.split_whitespace().all(|token| {
        token
            .strip_prefix('-')
            .filter(|token| !token.is_empty())
            .map_or_else(
                || value.contains(&token.to_lowercase()),
                |token| !value.contains(&token.to_lowercase()),
            )
    })
}

fn install_theme() {
    let path = config::configured_path();
    let (mut observed, css) = read_theme(path.as_deref());
    let provider = CssProvider::new();
    provider.load_from_data(&css);
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("GTK display is available"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let Some(path) = path else {
        return;
    };
    glib::timeout_add_local(THEME_RELOAD_INTERVAL, move || {
        let Ok(source) = fs::read_to_string(&path) else {
            return glib::ControlFlow::Continue;
        };
        if observed.as_deref() == Some(source.as_str()) {
            return glib::ControlFlow::Continue;
        }
        observed = Some(source.clone());
        match config::parse(&source) {
            Ok(theme) => provider.load_from_data(&theme.css),
            Err(error) => eprintln!(
                "rofi-preview-shared: ignoring invalid CSS theme {}: {error}",
                path.display()
            ),
        }
        glib::ControlFlow::Continue
    });
}

fn launcher_geometry() -> config::WindowConfig {
    config::configured_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|source| config::parse(&source).ok())
        .unwrap_or_else(config::embedded)
        .window
}

fn read_theme(path: Option<&Path>) -> (Option<String>, String) {
    let Some(path) = path else {
        return (None, config::embedded().css);
    };
    match fs::read_to_string(path) {
        Ok(source) => match config::parse(&source) {
            Ok(theme) => (Some(source), theme.css),
            Err(error) => {
                eprintln!(
                    "rofi-preview-shared: ignoring invalid CSS theme {}: {error}",
                    path.display()
                );
                (Some(source), config::embedded().css)
            }
        },
        Err(_) => (None, config::embedded().css),
    }
}

#[cfg(test)]
mod launcher_tests {
    use super::*;

    #[test]
    fn token_matching_requires_each_substring() {
        assert!(token_matches("Rust clipboard manager", "rust board"));
        assert!(!token_matches("Rust clipboard manager", "rust missing"));
    }

    #[test]
    fn token_matching_supports_exclusions_without_fuzzy_reordering() {
        assert!(token_matches("clipboard memo", "clip -text"));
        assert!(!token_matches("clipboard text", "clip -text"));
        assert!(!token_matches("clipboard", "clp"));
    }

    #[test]
    fn permanent_rows_remain_visible_and_last_while_filtering() {
        let mut matching = Row::new("match", "matching text");
        matching.search_text = "matching text".to_owned();
        let mut draft = Row::new("draft", "New memo");
        draft.search_text.clear();
        draft.permanent = true;

        let visible = visible_rows(&[matching, draft], "match");
        assert_eq!(visible, [0, 1]);
    }

    #[test]
    fn filtering_preserves_source_order() {
        let rows = [
            Row::new("one", "match at the end"),
            Row::new("two", "match first"),
        ];
        assert_eq!(visible_rows(&rows, "match"), [0, 1]);
    }
}
