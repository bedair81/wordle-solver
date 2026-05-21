mod tui;

fn main() {
    if let Err(err) = tui::run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}
