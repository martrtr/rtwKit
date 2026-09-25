//! CLI for validating, building, and publishing rtwKit package metadata.

mod model;
mod package;
mod registry;

use std::{env, path::PathBuf};

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let mut arguments = env::args().skip(1);
    let Some(command) = arguments.next() else {
        print_usage();
        bail!("missing command");
    };

    match command.as_str() {
        "validate-repository" => {
            let repository = required_path(&mut arguments, "repository")?;
            reject_extra(arguments)?;
            package::validate_repository(&repository)?;
        }
        "build-package" => build_package_command(arguments)?,
        "build-index" => {
            let metadata = required_path(&mut arguments, "metadata directory")?;
            let output = required_path(&mut arguments, "index output")?;
            reject_extra(arguments)?;
            registry::build_index(&metadata, &output)?;
        }
        _ => {
            print_usage();
            bail!("unknown command '{command}'");
        }
    }
    Ok(())
}

fn build_package_command(mut arguments: impl Iterator<Item = String>) -> Result<()> {
    let repository = required_path(&mut arguments, "repository")?;
    let slug = required_string(&mut arguments, "package slug")?;
    let output = required_path(&mut arguments, "output directory")?;
    let release_repository = required_string(&mut arguments, "release repository")?;
    let release_tag = required_string(&mut arguments, "release tag")?;
    let source_commit = required_string(&mut arguments, "source commit")?;
    reject_extra(arguments)?;

    let (artifact, metadata) = package::build_package(
        &repository,
        &slug,
        &output,
        &release_repository,
        &release_tag,
        &source_commit,
    )?;
    println!("{}", artifact.display());
    println!("{}", metadata.display());
    Ok(())
}

fn required_path(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(required_string(arguments, name)?))
}

fn required_string(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<String> {
    arguments
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing {name}"))
}

fn reject_extra(mut arguments: impl Iterator<Item = String>) -> Result<()> {
    if let Some(argument) = arguments.next() {
        bail!("unexpected extra argument '{argument}'");
    }
    Ok(())
}

fn print_usage() {
    eprintln!("rtwkit-registry-tool validate-repository <repository>");
    eprintln!(
        "rtwkit-registry-tool build-package <repository> <slug> <output> \\\n<owner/repo> <release-tag> <source-commit>"
    );
    eprintln!("rtwkit-registry-tool build-index <metadata-directory> <output>");
}
