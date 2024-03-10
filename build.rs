use std::io;

fn main() -> io::Result<()> {
    if cfg!(target_os = "windows") {
        winres::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()?;
    }
    Ok(())
}
