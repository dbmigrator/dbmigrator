fn main() -> std::io::Result<()> {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target_os == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icons/dbmigrator.ico")
            .set("ProductName", "DBMigrator")
            .set("FileDescription", "DBMigrator CLI")
            .set("LegalCopyright", "(C) DBMigrator. All rights reserved");
        res.compile()?;
    }
    Ok(())
}
