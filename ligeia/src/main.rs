use druid::{
    menu, widget::TextBox, AppLauncher, Data, Lens, LocalizedString, Menu, Widget, WidgetExt,
    WindowDesc,
};

const TEXT_BOX_WIDTH: f64 = 200.0;

#[derive(Clone, Data, Lens)]
struct State {
    name: String,
}

fn main() {
    let main_window = WindowDesc::new(build_root_widget())
        .title("Ligeia")
        .menu(|_, _, _| menu_bar());

    let initial_state = State {
        name: "Foobar".to_string(),
    };

    AppLauncher::with_window(main_window)
        .log_to_console()
        .launch(initial_state)
        .expect("failed to launch application");
}

fn build_root_widget() -> impl Widget<State> {
    // a textbox that modifies `name`.
    let textbox = TextBox::new()
        .with_placeholder("Who are we greeting?")
        .with_text_size(18.0)
        .fix_width(TEXT_BOX_WIDTH)
        .lens(State::name);

    textbox
}

fn menu_bar<T: Data>() -> Menu<T> {
    use menu::sys::mac::{application, file};
    Menu::new(LocalizedString::new(""))
        .entry(
            Menu::new(LocalizedString::new("application-menu"))
                .entry(application::about())
                .entry(application::preferences())
                .entry(application::quit()),
        )
        .entry(Menu::new(LocalizedString::new("File")).entry(file::open_file()))
}
