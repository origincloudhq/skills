//! Reads the API definitions: compiles every file under the API's
//! `origincloud/` with its pinned dependencies, and finds the newest package
//! of each service. The same scheme as origincloudhq/mcp's generator, so
//! that both see the same services.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use prost_reflect::DescriptorPool;
use prost_types::FileDescriptorSet;
use protox::file::{
    ChainFileResolver, File, FileResolver, GoogleFileResolver, IncludeFileResolver,
};

/// The service whose package holds every service's operation methods:
/// `origincloud.longrunning.{version}`. Each service serves them at its own
/// host, for the operations its methods answer with.
pub const OPERATIONS: &str = "longrunning";

/// Compiles every file under the API's `origincloud/`, with source info
/// and the files they import.
pub fn compile(
    api: &Path,
    dependencies: BTreeMap<String, String>,
) -> Result<(DescriptorPool, FileDescriptorSet)> {
    let mut resolver = ChainFileResolver::new();
    resolver.add(IncludeFileResolver::new(api.to_owned()));
    resolver.add(MemoryResolver(dependencies));
    resolver.add(GoogleFileResolver::new());
    let mut compiler = protox::Compiler::with_file_resolver(resolver);
    compiler.include_imports(true).include_source_info(true);

    let mut names = Vec::new();
    collect_protos(api, &api.join("origincloud"), &mut names)?;
    names.sort();
    ensure!(
        !names.is_empty(),
        "no .proto files under {}",
        api.join("origincloud").display()
    );
    compiler
        .open_files(&names)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    Ok((compiler.descriptor_pool(), compiler.file_descriptor_set()))
}

fn collect_protos(root: &Path, dir: &Path, names: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_protos(root, &path, names)?;
        } else if path.extension().is_some_and(|e| e == "proto") {
            let relative = path.strip_prefix(root)?;
            let name: Vec<String> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            names.push(name.join("/"));
        }
    }
    Ok(())
}

/// The downloaded dependencies, by import path.
struct MemoryResolver(BTreeMap<String, String>);

impl FileResolver for MemoryResolver {
    fn open_file(&self, name: &str) -> Result<File, protox::Error> {
        match self.0.get(name) {
            Some(source) => File::from_source(name, source),
            None => Err(protox::Error::file_not_found(name)),
        }
    }
}

/// The newest package of every service of the API but the operations one,
/// by service: `pvs` is `origincloud.pvs.v1beta1`. Every file under
/// `origincloud/` must be in a package `origincloud.{service}.{version}`:
/// any other shape fails generation, rather than leaving a service out
/// unnoticed.
pub fn services(pool: &DescriptorPool) -> Result<BTreeMap<String, String>> {
    let mut names: Vec<String> = Vec::new();
    for file in pool
        .files()
        .filter(|f| f.name().starts_with("origincloud/"))
    {
        let package = file.package_name();
        let service = service_of_package(package).with_context(|| {
            format!(
                "{} is in the package {package:?}, not one shaped origincloud.{{service}}.{{version}} \
                 (such as origincloud.pvs.v1beta1)",
                file.name()
            )
        })?;
        if service != OPERATIONS && file.services().next().is_some() {
            names.push(service.to_owned());
        }
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|service| Ok((service.clone(), newest_package(pool, &service)?)))
        .collect()
}

/// The service of a package shaped `origincloud.{service}.{version}`, the
/// service a host label (`[a-z][a-z0-9]*`) and the version one
/// [`version_key`] orders; `None` for any other shape.
pub fn service_of_package(package: &str) -> Option<&str> {
    let mut parts = package.split('.');
    let (Some("origincloud"), Some(service), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let label = service.starts_with(|c: char| c.is_ascii_lowercase())
        && service
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    (label && version_key(version).is_some()).then_some(service)
}

/// The newest version of `origincloud.{service}`: a stable version before
/// a beta, a beta before an alpha, and a higher number first.
pub fn newest_package(pool: &DescriptorPool, service: &str) -> Result<String> {
    let prefix = format!("origincloud.{service}.");
    let mut versions: Vec<(VersionKey, String)> = Vec::new();
    for file in pool.files() {
        let package = file.package_name();
        if let Some(version) = package.strip_prefix(&prefix) {
            let key = version_key(version)
                .with_context(|| format!("the package {package} has no version"))?;
            if !versions.iter().any(|(_, p)| p == package) {
                versions.push((key, package.to_owned()));
            }
        }
    }
    versions.sort();
    versions
        .pop()
        .map(|(_, package)| package)
        .with_context(|| format!("the API has no origincloud.{service} package"))
}

/// Orders versions: (major, stability, minor), stability 0 for alpha, 1 for
/// beta and 2 for stable.
type VersionKey = (u32, u8, u32);

fn version_key(version: &str) -> Option<VersionKey> {
    let rest = version.strip_prefix('v')?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let major = rest[..digits].parse().ok()?;
    let rest = &rest[digits..];
    let (stability, minor) = if rest.is_empty() {
        (2, 0)
    } else if let Some(n) = rest.strip_prefix("beta") {
        (1, n.parse().ok()?)
    } else {
        (0, rest.strip_prefix("alpha")?.parse().ok()?)
    };
    Some((major, stability, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_stable_before_beta_before_alpha() {
        let mut keys = ["v1beta1", "v1", "v1alpha2", "v2beta1", "v1beta2"]
            .map(|v| (version_key(v).unwrap(), v));
        keys.sort();
        let order: Vec<&str> = keys.iter().map(|(_, v)| *v).collect();
        assert_eq!(order, ["v1alpha2", "v1beta1", "v1beta2", "v1", "v2beta1"]);
        assert!(version_key("latest").is_none());
    }

    #[test]
    fn a_package_is_origincloud_service_version() {
        assert_eq!(service_of_package("origincloud.pvs.v1beta1"), Some("pvs"));
        for bad in [
            "origincloud.pvs",
            "origincloud.pvs.v1beta1.internal",
            "origincloud.Pvs.v1",
            "google.pvs.v1",
        ] {
            assert_eq!(service_of_package(bad), None, "{bad}");
        }
    }
}
