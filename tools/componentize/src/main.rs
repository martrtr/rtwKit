//! CLI for wrapping core WebAssembly modules as validated WebAssembly Components.

use std::{env, fs};

use anyhow::{Context, Result};
use wit_component::ComponentEncoder;

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .context("missing input core WebAssembly module")?;
    let output = args.next().context("missing output Component Model path")?;
    if args.next().is_some() {
        anyhow::bail!("usage: rtwkit-componentize <core.wasm> <component.wasm>");
    }

    let module = fs::read(&input).with_context(|| format!("read core module {input:?}"))?;
    let component = ComponentEncoder::default()
        .module(&module)?
        .validate(true)
        .encode()?;
    fs::write(&output, component).with_context(|| format!("write component {output:?}"))?;
    Ok(())
}
