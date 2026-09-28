fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let manifest = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: check-pack MANIFEST_JSON"))?;
    anyhow::ensure!(args.next().is_none(), "expected one manifest path");
    let pack = avatar_pack::open(std::path::Path::new(&manifest))?;
    pack.load_model()?;
    println!(
        "validated {} (schema v{})",
        pack.manifest.display_name, pack.manifest.schema_version
    );
    Ok(())
}
