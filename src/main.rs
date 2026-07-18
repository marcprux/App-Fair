fn main() {
    day::launch(
        day::WindowOptions {
            title: "App Fair".into(),
            // A desktop-appropriate default size; mobile fills the screen regardless.
            size: day::prelude::Size::new(960.0, 640.0),
            ..Default::default()
        },
        app_fair::root,
    );
}
