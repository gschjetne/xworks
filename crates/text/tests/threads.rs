//! Fonts, hyphenation patterns, tab stops and layouts go between threads,
//! as the app lays paragraphs out on every core against one set of fonts
//! (docs/ARCHITECTURE.md, section 8); the compiler checks it.

#[test]
fn fonts_and_layouts_go_between_threads() {
    fn shared<T: Send + Sync + 'static>() {}
    shared::<text::Fonts>();
    shared::<text::Hyphenation>();
    shared::<text::Hyphenator>();
    shared::<text::Tabs>();
    shared::<text::Layout>();
    shared::<text::Style>();
}
