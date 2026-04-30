use crate::theme::ThemeColors;
use crate::ui::{keys, lighting, performance, profiles};
use crate::AppShared;
use adw::prelude::*;
use relm4::prelude::*;
use std::sync::mpsc;

pub struct AppModel {
    shared: AppShared,
    toast_overlay: adw::ToastOverlay,
    css_provider: gtk::CssProvider,
}

#[derive(Debug)]
pub enum AppMsg {
    ThemeChanged(ThemeColors),
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = AppShared;
    type Input = AppMsg;
    type Output = ();

    view! {
        #[root]
        adw::ApplicationWindow {
            set_title: Some("CNTRL"),
            set_default_width: 1240,
            set_default_height: 860,
        }
    }

    fn init(init: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let toast_overlay = adw::ToastOverlay::new();
        let css_provider = gtk::CssProvider::new();

        let model = AppModel {
            shared: init,
            toast_overlay,
            css_provider,
        };

        let widgets = view_output!();

        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &model.css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let title = gtk::Label::new(Some("CNTRL Native"));
        header.set_title_widget(Some(&title));

        let profile_switcher = gtk::DropDown::from_strings(&["Profile 1", "Profile 2", "Profile 3", "Profile 4"]);
        {
            let shared = model.shared.clone();
            let sender = sender.input_sender().clone();
            profile_switcher.connect_selected_notify(move |dd| {
                let slot = dd.selected() as u8;
                if slot <= 3 {
                    if let Ok(next) = crate::load_profile_slot(slot) {
                        if let Ok(mut active) = shared.active_slot.lock() {
                            *active = slot;
                        }
                        if let Ok(mut p) = shared.profile.lock() {
                            *p = next;
                        }
                        let _ = crate::persist_active_slot(slot);
                    }
                }
                let _ = sender.send(AppMsg::ThemeChanged(crate::theme::load_theme_from_disk()));
            });
        }
        header.pack_start(&profile_switcher);
        toolbar.add_top_bar(&header);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let tab_view = adw::TabView::new();

        let lighting_page = tab_view.add_page(&lighting::page(model.shared.clone(), model.toast_overlay.clone()), None::<&adw::TabPage>);
        lighting_page.set_title("Lighting");

        let perf_page = tab_view.add_page(&performance::page(), None::<&adw::TabPage>);
        perf_page.set_title("Performance");

        let keys_page = tab_view.add_page(&keys::page(), None::<&adw::TabPage>);
        keys_page.set_title("Keys");

        let profiles_page = tab_view.add_page(&profiles::page(), None::<&adw::TabPage>);
        profiles_page.set_title("Profiles");

        let tab_bar = adw::TabBar::new();
        tab_bar.set_view(Some(&tab_view));
        content.append(&tab_bar);
        content.append(&tab_view);

        toolbar.set_content(Some(&content));
        model.toast_overlay.set_child(Some(&toolbar));
        root.set_content(Some(&model.toast_overlay));

        let (tx, rx) = mpsc::channel::<ThemeColors>();
        let theme_state = model.shared.theme_colors.clone();
        crate::theme::start_theme_watcher(theme_state, move |fresh| {
            let _ = tx.send(fresh);
        });

        let sender_clone = sender.input_sender().clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            while let Ok(theme) = rx.try_recv() {
                let _ = sender_clone.send(AppMsg::ThemeChanged(theme));
            }
            gtk::glib::ControlFlow::Continue
        });

        if let Ok(theme) = model.shared.theme_colors.lock().map(|t| t.clone()) {
            model.apply_theme_css(&theme);
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            AppMsg::ThemeChanged(theme) => {
                if let Ok(mut t) = self.shared.theme_colors.lock() {
                    *t = theme.clone();
                }
                self.apply_theme_css(&theme);
                self.toast_overlay
                    .add_toast(adw::Toast::new("Theme synced -> keyboard updated"));
            }
        }
    }
}

impl AppModel {
    fn apply_theme_css(&self, theme: &ThemeColors) {
        let css = format!(
            r#"
* {{ --accent-color: {}; }}
@define-color accent_color {};

/* Cards & Containers */
.card {{
    background-color: @view_bg_color;
    border: 1px solid @borders;
    border-radius: 8px;
    padding: 16px;
    box-shadow: 0 1px 3px rgba(0,0,0,0.08);
}}

.card-title {{
    font-weight: 500;
    font-size: 13px;
    color: @theme_fg_color;
    margin-bottom: 4px;
}}

/* Control Labels */
.control-label {{
    font-weight: 400;
    font-size: 11px;
    color: @theme_fg_color;
    text-transform: uppercase;
    letter-spacing: 0.5px;
}}

.color-hex {{
    font-family: monospace;
    font-size: 14px;
    font-weight: 600;
    color: @accent_color;
}}

.swatch-label {{
    font-weight: 500;
    font-size: 11px;
    color: @theme_fg_color;
    margin-top: 8px;
}}

/* Sliders with subtle styling */
.slider {{
    margin-top: 4px;
    margin-bottom: 4px;
}}

.slider trough {{
    min-height: 4px;
    border-radius: 2px;
    background-color: @wm_bg;
}}

.slider highlight {{
    background-color: @accent_color;
    border-radius: 2px;
}}

.slider slider {{
    min-width: 16px;
    min-height: 16px;
    margin: -6px 0;
    background-color: @accent_color;
    border-radius: 8px;
    border: 2px solid @view_bg_color;
    box-shadow: 0 2px 4px rgba(0,0,0,0.12);
    transition: all 150ms ease-out;
}}

.slider slider:hover {{
    box-shadow: 0 4px 8px rgba(0,0,0,0.16);
}}

.slider slider:active {{
    transform: scale(1.1);
    box-shadow: 0 4px 12px rgba(0,0,0,0.2);
}}

/* Effect Pills */
.effect-pill {{
    padding: 12px 16px;
    font-size: 12px;
    font-weight: 500;
    border-radius: 20px;
    border: 1px solid @borders;
    background-color: @view_bg_color;
    color: @theme_fg_color;
    cursor: pointer;
    transition: all 150ms ease-out;
    margin: 4px;
}}

.effect-pill:hover {{
    background-color: @view_hover_bg_color;
    border-color: @accent_color;
    box-shadow: 0 2px 6px rgba(0,0,0,0.12);
}}

.effect-pill:active, .effect-pill:checked {{
    background-color: @accent_color;
    color: @view_bg_color;
    border-color: @accent_color;
    box-shadow: 0 2px 8px rgba(0,0,0,0.2);
}}

.effect-pill:focus-visible {{
    outline: 2px solid @accent_color;
    outline-offset: 2px;
}}

/* Effects Grid */
.effects-grid {{
    padding: 8px;
}}

/* Swatches */
.swatch {{
    transition: transform 150ms ease-out, box-shadow 150ms ease-out;
}}

.swatch:hover {{
    transform: scale(1.05);
    box-shadow: 0 2px 6px rgba(0,0,0,0.2) !important;
}}

.swatch:active {{
    transform: scale(0.95);
}}

/* Color Picker */
.picker {{
    border: 1px solid @borders;
    border-radius: 6px;
    background-color: @view_bg_color;
}}

/* Apply Button */
.apply-button {{
    font-size: 14px;
    font-weight: 500;
    padding: 12px 24px;
    border-radius: 6px;
    transition: all 150ms ease-out;
}}

.apply-button:hover {{
    box-shadow: 0 4px 12px rgba(0,0,0,0.15);
}}

.apply-button:active {{
    transform: translateY(1px);
    box-shadow: 0 2px 6px rgba(0,0,0,0.12);
}}

/* Tab styling */
tabbar {{
    background-color: transparent;
}}

tabbar tab {{
    font-size: 13px;
    font-weight: 500;
    margin: 8px 12px;
    padding: 4px 8px;
    border-radius: 6px;
    transition: all 150ms ease-out;
}}

tabbar tab:hover {{
    background-color: @wm_bg;
}}

tabbar tab:checked {{
    border-bottom: 3px solid @accent_color;
    color: @accent_color;
}}
"#,
            theme.accent, theme.accent
        );
        self.css_provider.load_from_string(&css);
    }
}
