//! Generates the skills' references and the generated parts of their
//! `SKILL.md` from the API definitions (origincloudhq/api) and the pinned
//! client releases:
//!
//! ```text
//! origincloud-skills-generator <repository> <inputs> <out-dir>
//! ```
//!
//! `<repository>` is this repository: its `API_REVISION`, `clients.toml`,
//! `generator/docs.toml`, `shared/rules.md` and the hand-written
//! `SKILL.md` of each skill. `<inputs>` is what `scripts/generate.sh`
//! fetched for them:
//!
//! - `api/`: the API at `API_REVISION`; its dependencies are downloaded at
//!   the commits its `buf.lock` pins;
//! - `tools.toml`: origincloudhq/mcp's table at the pinned commit, whose
//!   `[[one_way]]` list is the one list of changes that can't be undone;
//! - `commands.json`: the CLI release's command tree;
//! - `terraform.json` and `terraform-api-revision`: the provider release's
//!   schema, and the API commit it was generated from;
//! - `sdk-rust/`, `sdk-go/`, `sdk-typescript/`: the SDK releases' sources.
//!
//! The files are written to `<out-dir>` at their paths in this repository:
//! `skills/{skill}/SKILL.md` and `skills/{skill}/references/{service}.md`
//! for every service of the API. Generation fails when a client generated
//! from the pinned API lacks a name the references would use, or when a
//! skill's files name a command, flag or type its client doesn't have.

mod api;
mod cli;
mod deps;
mod docs;
mod model;
mod pins;
mod policy;
mod render;
mod sdk;
mod skill;
mod terraform;
mod verify;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};

fn main() -> Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [root, inputs, out] = args.as_slice() else {
        bail!("usage: origincloud-skills-generator <repository> <inputs> <out-dir>");
    };
    let files = generate(root, inputs)?;
    for (path, content) in &files {
        let path = out.join(path);
        std::fs::create_dir_all(path.parent().context("a file has a directory")?)?;
        std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// Every generated file, by its path in the repository.
fn generate(root: &Path, inputs: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let api_revision = read(&root.join("API_REVISION"))?.trim().to_owned();
    let clients = pins::Clients::load(&root.join("clients.toml"))?;
    let docs = pins::Docs::load(&root.join("generator/docs.toml"))?;
    let rules = read(&root.join("shared/rules.md"))?;
    let policy = policy::Policy::load(&inputs.join("tools.toml"))?;

    let api_dir = inputs.join("api");
    let dependencies = deps::fetch(&api_dir)?;
    let (pool, fds) = api::compile(&api_dir, dependencies)?;
    let comments = docs::Comments::new(&fds);
    let packages = api::services(&pool)?;
    ensure!(!packages.is_empty(), "the API has no origincloud service");
    let services: Vec<model::Service> = packages
        .iter()
        .map(|(name, package)| model::service(&pool, &comments, &policy, name, package))
        .collect();
    let methods = model::methods(&services);
    for o in &policy.one_way {
        if !methods.keys().any(|m| {
            m.ends_with(&format!(".{}", o.method))
                && m.starts_with(&format!("origincloud.{}.", o.service))
        }) {
            eprintln!(
                "warning: origincloudhq/mcp's [[one_way]] names {} {}, which the API doesn't have",
                o.service, o.method
            );
        }
    }
    for s in &services {
        if docs.markdown(&s.name).is_none() {
            eprintln!(
                "warning: generator/docs.toml names no guide for {}; its references link to none",
                s.name
            );
        }
    }

    let tree = cli::Tree::load(&inputs.join("commands.json"))?;
    ensure!(
        tree.cli_version == clients.cli.version,
        "commands.json is origin {}'s, but clients.toml pins {}",
        tree.cli_version,
        clients.cli.version
    );
    let provider = terraform::load(&inputs.join("terraform.json"), &clients.terraform.source)?;
    let terraform_api_revision = std::fs::read_to_string(inputs.join("terraform-api-revision"))
        .ok()
        .map(|s| s.trim().to_owned());
    let versions: Vec<(String, String)> = services
        .iter()
        .map(|s| (s.name.clone(), s.version.clone()))
        .collect();
    let mut sdks = BTreeMap::new();
    for language in sdk::LANGUAGES {
        let version = clients
            .sdk
            .get(language.key())
            .with_context(|| format!("clients.toml pins no sdk.{}", language.key()))?;
        let release = sdk::Release::load(
            language,
            version,
            &inputs.join(format!("sdk-{}", language.key())),
            &versions,
        )?;
        sdks.insert(language, release);
    }

    let inputs = render::Inputs {
        api_revision: &api_revision,
        clients: &clients,
        docs: &docs,
        cli: &tree,
        terraform: &provider,
        terraform_api_revision: terraform_api_revision.as_deref(),
        sdks: &sdks,
    };

    let mut problems: render::Problems = Vec::new();
    let mut files: BTreeMap<PathBuf, String> = BTreeMap::new();
    for s in &services {
        let references = [
            (render::ORIGIN, render::origin(&inputs, s, &mut problems)),
            (
                render::TERRAFORM,
                render::terraform(&inputs, s, &mut problems),
            ),
            (render::SDK, render::sdk(&inputs, s, &mut problems)),
        ];
        for (skill, text) in references {
            files.insert(
                PathBuf::from(format!("skills/{skill}/references/{}.md", s.name)),
                text,
            );
        }
    }

    let rules_block = rules_block(&rules);
    for skill in render::SKILLS {
        let dir = root.join("skills").join(skill);
        let workflows = workflows(&dir.join("references"));
        let mut blocks = BTreeMap::new();
        blocks.insert("rules", rules_block.clone());
        blocks.insert("references", render::reference_map(&services, &workflows));
        let text = read(&dir.join("SKILL.md"))?;
        let spliced = skill::splice(&text, &metadata(skill, &api_revision, &clients), &blocks)
            .with_context(|| format!("skills/{skill}/SKILL.md"))?;
        skill::validate(skill, &spliced)?;
        files.insert(PathBuf::from(format!("skills/{skill}/SKILL.md")), spliced);
    }

    // The names every skill file uses, generated or hand-written.
    let mut texts: BTreeMap<PathBuf, String> = BTreeMap::new();
    for skill in render::SKILLS {
        collect_markdown(&root.join("skills").join(skill), root, &mut texts)?;
    }
    for (path, text) in &files {
        texts.insert(path.clone(), text.clone());
    }
    for (path, text) in &texts {
        let file = path.display().to_string();
        if file.starts_with(&format!("skills/{}/", render::ORIGIN)) {
            problems.extend(verify::cli(&file, text, &tree));
        }
        if file.starts_with(&format!("skills/{}/", render::TERRAFORM)) {
            problems.extend(verify::terraform(
                &file,
                text,
                &provider,
                &clients.terraform.version,
            ));
        }
    }
    if !problems.is_empty() {
        for p in &problems {
            eprintln!("error: {p}");
        }
        bail!("{} names don't match the pinned clients", problems.len());
    }
    Ok(files)
}

/// The shared rules as a `SKILL.md` block: `shared/rules.md` without its
/// title, its headings a level lower.
fn rules_block(rules: &str) -> String {
    let mut out = String::new();
    for line in rules.lines() {
        if line.starts_with("# ") {
            continue;
        }
        if line.starts_with('#') {
            out.push('#');
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_owned() + "\n"
}

/// The versions a skill describes, for its frontmatter's `metadata`.
fn metadata(skill: &str, api_revision: &str, clients: &pins::Clients) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("api-revision".to_owned(), api_revision.to_owned());
    match skill {
        render::ORIGIN => {
            m.insert("cli-version".to_owned(), clients.cli.version.clone());
        }
        render::TERRAFORM => {
            m.insert(
                "provider-version".to_owned(),
                clients.terraform.version.clone(),
            );
        }
        _ => {
            for (language, version) in &clients.sdk {
                m.insert(format!("sdk-{language}-version"), version.clone());
            }
        }
    }
    m
}

/// The services with a hand-written `{service}-workflows.md`.
fn workflows(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_suffix("-workflows.md")
                .map(str::to_owned)
        })
        .collect();
    out.sort();
    out
}

fn collect_markdown(dir: &Path, root: &Path, out: &mut BTreeMap<PathBuf, String>) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_markdown(&path, root, out)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            out.insert(path.strip_prefix(root)?.to_owned(), read(&path)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The google/api files the tests compile against, by import path, so
    /// that the tests need no download.
    fn fixtures() -> BTreeMap<String, String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/google/api");
        std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| {
                let path = e.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (
                    format!("google/api/{name}"),
                    std::fs::read_to_string(&path).unwrap(),
                )
            })
            .collect()
    }

    const KEYS: &str = r#"
        syntax = "proto3";
        package origincloud.keys.v1;
        import "google/api/annotations.proto";
        import "google/api/client.proto";
        import "google/api/field_behavior.proto";
        import "google/api/resource.proto";
        // Keys of a project.
        service KeyService {
          option (google.api.default_host) = "keys.origincloudapis.eu";
          // Returns a key.
          rpc GetKey(GetKeyRequest) returns (Key) {
            option (google.api.http) = {get: "/v1/{name=projects/*/keys/*}"};
          }
          // Deletes a key.
          rpc DeleteKey(DeleteKeyRequest) returns (Key) {
            option (google.api.http) = {delete: "/v1/{name=projects/*/keys/*}"};
          }
          // Lengthens a key. It never gets shorter.
          rpc LengthenKey(LengthenKeyRequest) returns (Key) {
            option (google.api.http) = {post: "/v1/{name=projects/*/keys/*}:lengthen" body: "*"};
          }
        }
        // A key.
        message Key {
          option (google.api.resource) = {
            type: "keys.origincloudapis.eu/Key"
            pattern: "projects/{project}/keys/{key}"
            plural: "keys"
            singular: "key"
          };
          // The key's resource name.
          string name = 1 [(google.api.field_behavior) = IDENTIFIER];
          // Its length in bits.
          int32 bits = 2 [(google.api.field_behavior) = REQUIRED, (google.api.field_behavior) = IMMUTABLE];
          // When it was made.
          string created = 3 [(google.api.field_behavior) = OUTPUT_ONLY];
        }
        message GetKeyRequest { string name = 1 [(google.api.field_behavior) = REQUIRED]; }
        message DeleteKeyRequest {
          string name = 1 [(google.api.field_behavior) = REQUIRED];
          string request_id = 2;
        }
        message LengthenKeyRequest {
          string name = 1 [(google.api.field_behavior) = REQUIRED];
          int32 bits = 2;
        }
    "#;

    const COMMANDS: &str = r#"{"cli_version":"1.0.0","api_revision":"abc",
        "commands":[
          {"path":["keys","get-key"],"method":"origincloud.keys.v1.KeyService.GetKey","args":[{"long":"key","required":true,"value_name":"KEY"}]},
          {"path":["keys","delete-key"],"method":"origincloud.keys.v1.KeyService.DeleteKey","args":[{"long":"key","required":true},{"long":"yes"}]}
        ],
        "global_args":[{"long":"output","short":"o"}]}"#;

    const SCHEMA: &str = r#"{"provider_schemas":{"registry.opentofu.org/origincloudhq/origincloud":{
        "resource_schemas":{"origincloud_keys_key":{"block":{"attributes":{
          "bits":{"description":"Its length in bits. Changing it replaces the key.","required":true},
          "name":{"description":"The key's resource name.","computed":true}}}}},
        "data_source_schemas":{"origincloud_keys_keys":{"block":{"attributes":{}}}}}}}"#;

    struct Fixture {
        services: Vec<model::Service>,
        clients: pins::Clients,
        docs: pins::Docs,
        tree: cli::Tree,
        provider: terraform::Provider,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("origincloud/keys/v1/keys.proto");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, KEYS).unwrap();
        let (pool, fds) = api::compile(dir.path(), fixtures()).unwrap();
        let comments = docs::Comments::new(&fds);
        let policy: policy::Policy = toml::from_str(
            "[[one_way]]\nservice = \"keys\"\nmethod = \"LengthenKey\"\nfield = \"bits\"\nconsequence = \"A key never gets shorter.\"\n",
        )
        .unwrap();
        let services = vec![model::service(
            &pool,
            &comments,
            &policy,
            "keys",
            "origincloud.keys.v1",
        )];
        let clients: pins::Clients = toml::from_str(
            "[mcp]\nrevision = \"x\"\n[cli]\nversion = \"1.0.0\"\ncommands = \"release\"\n\
             [terraform]\nsource = \"origincloudhq/origincloud\"\nversion = \"0.1.0\"\n[sdk]\n",
        )
        .unwrap();
        let docs: pins::Docs = toml::from_str(
            "base = \"https://docs.origincloud.eu/docs\"\n[guides]\nkeys = \"keys\"\n",
        )
        .unwrap();
        let schema = dir.path().join("schema.json");
        std::fs::write(&schema, SCHEMA).unwrap();
        Fixture {
            services,
            clients,
            docs,
            tree: serde_json::from_str(COMMANDS).unwrap(),
            provider: terraform::load(&schema, "origincloudhq/origincloud").unwrap(),
        }
    }

    fn inputs<'a>(
        f: &'a Fixture,
        sdks: &'a BTreeMap<sdk::Language, sdk::Release>,
        api: &'a str,
    ) -> render::Inputs<'a> {
        render::Inputs {
            api_revision: api,
            clients: &f.clients,
            docs: &f.docs,
            cli: &f.tree,
            terraform: &f.provider,
            terraform_api_revision: None,
            sdks,
        }
    }

    #[test]
    fn the_model_has_behaviors_deletes_and_one_way_changes() {
        let f = fixture();
        let s = &f.services[0];
        assert_eq!(s.host, "keys.origincloudapis.eu");
        let key = &s.resources[0];
        assert_eq!(
            key.full_names(),
            ["//keys.origincloudapis.eu/projects/{project}/keys/{key}"]
        );
        let bits = key.fields.iter().find(|f| f.name == "bits").unwrap();
        assert_eq!(bits.behavior_label(), "required, immutable");
        let methods = &s.interfaces[0].methods;
        let delete = methods.iter().find(|m| m.name == "DeleteKey").unwrap();
        assert!(delete.delete && delete.request_id && delete.destructive());
        let lengthen = methods.iter().find(|m| m.name == "LengthenKey").unwrap();
        assert_eq!(
            lengthen.one_way,
            [("bits".to_owned(), "A key never gets shorter.".to_owned())]
        );
        assert_eq!(
            lengthen.resource.as_deref(),
            Some("keys.origincloudapis.eu/Key")
        );
    }

    /// The CLI's names come from its command tree: a method without a
    /// command is listed as missing, and is a problem only when the release
    /// was generated from the pinned API.
    #[test]
    fn the_origin_reference_names_the_releases_commands() {
        let f = fixture();
        let sdks = BTreeMap::new();
        let mut problems = Vec::new();
        let text = render::origin(&inputs(&f, &sdks, "other"), &f.services[0], &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(text.contains("### `origin keys get-key`"), "{text}");
        assert!(text.contains("origin keys get-key --key <KEY>"), "{text}");
        assert!(
            text.contains("- `origin keys delete-key`: deletes for good."),
            "{text}"
        );
        assert!(
            text.contains("- `LengthenKey`, changing `bits`: A key never gets shorter."),
            "{text}"
        );
        assert!(text.contains("## Not in origin 1.0.0"), "{text}");
        assert!(
            text.contains("<https://docs.origincloud.eu/docs/keys/index.html.md>"),
            "{text}"
        );
        assert!(verify::cli("keys.md", &text, &f.tree).is_empty());

        render::origin(&inputs(&f, &sdks, "abc"), &f.services[0], &mut problems);
        assert_eq!(
            problems,
            ["origin 1.0.0 has no command for origincloud.keys.v1.KeyService.LengthenKey"]
        );
    }

    #[test]
    fn the_terraform_reference_names_the_schemas_types() {
        let f = fixture();
        let sdks = BTreeMap::new();
        let mut problems = Vec::new();
        let text = render::terraform(&inputs(&f, &sdks, "abc"), &f.services[0], &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(text.contains("- resource `origincloud_keys_key`"), "{text}");
        assert!(
            text.contains("- data source `origincloud_keys_keys`"),
            "{text}"
        );
        assert!(text.contains("| `bits` | required, replaces |"), "{text}");
        assert!(
            text.contains("id = \"projects/{project}/keys/{key}\""),
            "{text}"
        );
        assert!(
            text.contains("`origincloud_keys_key` destroyed or replaced: deletes for good."),
            "{text}"
        );
        assert!(verify::terraform("keys.md", &text, &f.provider, "0.1.0").is_empty());
        assert_eq!(
            verify::terraform("a.md", "`origincloud_keys_lock`", &f.provider, "0.1.0").len(),
            1
        );
    }
}
