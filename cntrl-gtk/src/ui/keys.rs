use gtk::prelude::*;

pub fn page() -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let label = gtk::Label::new(Some("Key remapping page coming next."));
    label.add_css_class("dim-label");
    label.set_margin_top(24);
    root.append(&label);
    root
}
