// Headless transport fixture for process lifecycle tests, not the renderer.
fn main() -> anyhow::Result<()> {
    pet_ipc::server::serve(
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        |_| Ok(()),
    )
}
