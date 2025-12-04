fn main() {
    if let Err(e) = dxid_tui::launch_tui() {
        eprintln!("tui failed: {e:?}");
    }
}
