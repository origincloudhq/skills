//! The SDKs' names for the API, by the naming rules of their generators
//! (origincloudhq/sdk-rust, sdk-go and sdk-typescript), and the check that
//! the pinned releases have them.
//!
//! - Rust: crate `origincloud-{service}`, module
//!   `origincloud_{service}::{version}`, `{Service}Client::new(&client)`,
//!   methods and fields in snake case, list helpers `all_{items}`.
//! - Go: package `go.origincloud.eu/sdk/{service}/{version}`, named
//!   `{service}{version}`, `New{Service}Client(client)`, methods as the
//!   RPCs, fields as protoc-gen-go names them, list helpers `All{Items}`.
//! - TypeScript: npm package `@origincloud/{service}`, imported from
//!   `@origincloud/{service}/{version}`, `new {Service}Client(client)`,
//!   methods in lower camel case, fields by their JSON names, list helpers
//!   `all{Items}`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use heck::ToSnakeCase;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Language {
    Rust,
    Go,
    TypeScript,
}

pub const LANGUAGES: [Language; 3] = [Language::Rust, Language::Go, Language::TypeScript];

impl Language {
    pub fn title(self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::Go => "Go",
            Language::TypeScript => "TypeScript",
        }
    }

    /// Its key in `clients.toml` and its repository's suffix: `sdk-{key}`.
    pub fn key(self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::Go => "go",
            Language::TypeScript => "typescript",
        }
    }

    /// The package a service's code is in, as a user installs it.
    pub fn package(self, service: &str) -> String {
        match self {
            Language::Rust => format!("origincloud-{service}"),
            Language::Go => "go.origincloud.eu/sdk".to_owned(),
            Language::TypeScript => format!("@origincloud/{service}"),
        }
    }

    /// The module a service version's types are imported from.
    pub fn module(self, service: &str, version: &str) -> String {
        match self {
            Language::Rust => format!("origincloud_{service}::{version}"),
            Language::Go => format!("go.origincloud.eu/sdk/{service}/{version}"),
            Language::TypeScript => format!("@origincloud/{service}/{version}"),
        }
    }

    /// The client type of a proto service: `VolumeClient` for
    /// `VolumeService`, the same in every language.
    pub fn client(self, interface: &str) -> String {
        format!(
            "{}Client",
            interface.strip_suffix("Service").unwrap_or(interface)
        )
    }

    /// How a service client is made from the core client `client`.
    pub fn constructor(self, service: &str, version: &str, interface: &str) -> String {
        let client = self.client(interface);
        match self {
            Language::Rust => format!("{client}::new(&client)"),
            Language::Go => format!("{service}{version}.New{client}(client)"),
            Language::TypeScript => format!("new {client}(client)"),
        }
    }

    /// The method of an RPC.
    pub fn method(self, rpc: &str) -> String {
        match self {
            Language::Rust => rust_ident(&rpc.to_snake_case()),
            Language::Go => rpc.to_owned(),
            Language::TypeScript => lower_first(rpc),
        }
    }

    /// The helper reading every page of a list, whose items are the
    /// response's repeated field `items` (`volumes`, JSON name `volumes`).
    pub fn all_pages(self, items: &str, items_json: &str) -> String {
        match self {
            Language::Rust => format!("all_{}", items.to_lowercase()),
            Language::Go => format!("All{}", go_camel(items)),
            Language::TypeScript => format!("all{}", upper_first(items_json)),
        }
    }

    /// The text in the SDK's source that shows a method is there.
    pub fn signature(self, interface: &str, rpc: &str) -> String {
        let client = self.client(interface);
        match self {
            Language::Rust => format!("fn {}(", self.method(rpc)),
            Language::Go => format!("func (c *{client}) {}(", self.method(rpc)),
            Language::TypeScript => format!("async {}(", self.method(rpc)),
        }
    }

    /// Where a service version's generated code is in the SDK's repository.
    pub fn source_dir(self, service: &str, version: &str) -> String {
        match self {
            Language::Rust => format!("crates/{service}/src"),
            Language::Go => format!("{service}/{version}"),
            Language::TypeScript => format!("packages/{service}/src/{version}"),
        }
    }
}

/// A Rust identifier: a keyword is escaped as prost does.
fn rust_ident(name: &str) -> String {
    match name {
        "self" | "super" | "crate" | "Self" => format!("{name}_"),
        "type" | "match" | "move" | "ref" | "fn" | "impl" | "use" | "mod" | "loop" | "where"
        | "async" | "await" | "dyn" | "static" | "const" | "trait" | "enum" | "struct" | "in"
        | "for" | "if" | "else" | "while" | "return" | "break" | "continue" | "let" | "mut"
        | "pub" | "true" | "false" | "as" | "extern" | "unsafe" | "box" | "yield" | "macro"
        | "abstract" | "become" | "do" | "final" | "override" | "priv" | "typeof" | "unsized"
        | "virtual" | "try" | "gen" => format!("r#{name}"),
        _ => name.to_owned(),
    }
}

/// protoc-gen-go's `GoCamelCase`: an underscore followed by a lower-case
/// letter starts a word, as does a letter after a digit.
pub fn go_camel(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_lowercase() {
            // A dot before a lower-case letter is dropped.
        } else if c == b'.' {
            out.push('_');
        } else if c == b'_' && (i == 0 || bytes[i - 1] == b'.') {
            out.push('X');
        } else if c == b'_' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_lowercase() {
            // An underscore before a lower-case letter is dropped.
        } else if c.is_ascii_digit() {
            out.push(c as char);
        } else {
            out.push(c.to_ascii_uppercase() as char);
            while i + 1 < bytes.len() && bytes[i + 1].is_ascii_lowercase() {
                i += 1;
                out.push(bytes[i] as char);
            }
        }
        i += 1;
    }
    out
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_ascii_lowercase().to_string() + c.as_str())
        .unwrap_or_default()
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
        .unwrap_or_default()
}

/// The pinned release of one SDK: its version, the API commit it was
/// generated from, and its generated source by service, to check the
/// names against.
pub struct Release {
    pub version: String,
    pub api_revision: String,
    sources: BTreeMap<String, String>,
}

impl Release {
    /// Reads the release checked out at `dir`, for the given services and
    /// their versions.
    pub fn load(
        language: Language,
        version: &str,
        dir: &Path,
        services: &[(String, String)],
    ) -> Result<Self> {
        let api_revision = std::fs::read_to_string(dir.join("API_REVISION"))
            .with_context(|| format!("reading API_REVISION of the {} SDK", language.title()))?
            .trim()
            .to_owned();
        let mut sources = BTreeMap::new();
        for (service, api_version) in services {
            let mut text = String::new();
            read_all(
                &dir.join(language.source_dir(service, api_version)),
                &mut text,
            );
            sources.insert(service.clone(), text);
        }
        Ok(Self {
            version: version.to_owned(),
            api_revision,
            sources,
        })
    }

    /// Whether the release has `rpc` of `interface` in `service`.
    pub fn has(&self, language: Language, service: &str, interface: &str, rpc: &str) -> bool {
        self.sources
            .get(service)
            .is_some_and(|s| s.contains(&language.signature(interface, rpc)))
    }
}

fn read_all(dir: &Path, out: &mut String) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            read_all(&path, out);
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            out.push_str(&text);
            out.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_each_generator() {
        assert_eq!(Language::Rust.method("CreateVolume"), "create_volume");
        assert_eq!(Language::Go.method("CreateVolume"), "CreateVolume");
        assert_eq!(Language::TypeScript.method("CreateVolume"), "createVolume");
        assert_eq!(Language::Rust.client("VolumeService"), "VolumeClient");
        assert_eq!(
            Language::Rust.all_pages("volumes", "volumes"),
            "all_volumes"
        );
        assert_eq!(Language::Go.all_pages("volumes", "volumes"), "AllVolumes");
        assert_eq!(
            Language::TypeScript.all_pages("audit_log_entries", "auditLogEntries"),
            "allAuditLogEntries"
        );
        assert_eq!(
            Language::Go.constructor("pvs", "v1beta1", "VolumeService"),
            "pvsv1beta1.NewVolumeClient(client)"
        );
    }

    #[test]
    fn go_names_are_protoc_gen_go_names() {
        assert_eq!(go_camel("size_gib"), "SizeGib");
        assert_eq!(go_camel("request_id"), "RequestId");
        assert_eq!(go_camel("ipv4_cidr_block"), "Ipv4CidrBlock");
        assert_eq!(go_camel("v4cidr"), "V4Cidr");
        assert_eq!(go_camel("audit_log_entries"), "AuditLogEntries");
    }
}
