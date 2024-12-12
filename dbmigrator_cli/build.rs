fn main() -> std::io::Result<()> {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target_os == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icons/dbmigrator.ico");
        res.compile()?;
    }
    Ok(())
}
