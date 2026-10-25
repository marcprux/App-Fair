fn main() {
    // The generated options carry the locale catalog, the localized title and the version; the
    // size is a desktop-appropriate default — mobile fills the screen regardless.
    day::launch(
        dayapp::res::window_options().size(960.0, 640.0),
        dayapp::root,
    );
}
