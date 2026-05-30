slint::include_modules!();
mod db;

fn main() {
    let window = AppWindow::new().unwrap();
    window.run().unwrap();
}
