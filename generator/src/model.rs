//! The resource model of one service, read from the API's descriptors: its
//! resources and their fields with their field behaviors, its methods, and
//! for each method whether it is long-running, destructive or one-way, and
//! whether it takes a `request_id`. Every renderer works from this model,
//! so that the skills say the same things about the API in each client's
//! names.

use std::collections::BTreeMap;

use prost_reflect::{
    DescriptorPool, DynamicMessage, FieldDescriptor, Kind, MessageDescriptor, MethodDescriptor,
    ServiceDescriptor, Value,
};

use crate::docs::{self, Comments};
use crate::policy::Policy;

// `google.api.FieldBehavior` values.
const OPTIONAL: i32 = 1;
const REQUIRED: i32 = 2;
const OUTPUT_ONLY: i32 = 3;
const INPUT_ONLY: i32 = 4;
const IMMUTABLE: i32 = 5;
const IDENTIFIER: i32 = 8;

/// One service of the API: the packages `origincloud.{name}.{version}`, at
/// their newest version.
#[derive(Debug)]
pub struct Service {
    /// `pvs`.
    pub name: String,
    /// `v1beta1`.
    pub version: String,
    /// `pvs.origincloudapis.eu`, the services' `google.api.default_host`.
    pub host: String,
    /// The proto services of the package, in file order.
    pub interfaces: Vec<Interface>,
    /// The resources the package declares, in file order.
    pub resources: Vec<Resource>,
    /// Whether any method answers with a long-running operation, which the
    /// service then serves at its own host.
    pub operations: bool,
}

/// A proto service, such as `VolumeService`.
#[derive(Debug)]
pub struct Interface {
    pub name: String,
    /// Its comment, cleaned.
    pub about: String,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind2 {
    Get,
    List,
    Create,
    Update,
    Delete,
    Custom,
}

/// A method of the API.
#[derive(Debug)]
pub struct Method {
    /// `CreateVolume`.
    pub name: String,
    /// `origincloud.pvs.v1beta1.VolumeService.CreateVolume`.
    pub full_name: String,
    pub kind: Kind2,
    /// Its comment, cleaned.
    pub about: String,
    /// The resource type it acts on, `pvs.origincloudapis.eu/Volume`, when
    /// one of the service's resources is its subject.
    pub resource: Option<String>,
    /// The request message, `CreateVolumeRequest`.
    pub request: String,
    /// The operation's result, `Volume`, for a long-running method.
    pub long_running: Option<String>,
    /// A delete: it removes something for good.
    pub delete: bool,
    /// The changes of it that can't be undone, from origincloudhq/mcp's
    /// `[[one_way]]` list: (request field, consequence).
    pub one_way: Vec<(String, String)>,
    /// The method has no MCP tool, with the reason, from origincloudhq/mcp's
    /// `[[no_tool]]` list: such as a method answering with a credential.
    pub no_tool: Option<String>,
    /// Its request takes a `request_id`, which makes a retry safe.
    pub request_id: bool,
    /// For a list answering in pages, the response's one repeated message
    /// field holding the items: its name and its JSON name (`volumes`).
    pub items: Option<(String, String)>,
    /// The request's fields at its top level.
    pub fields: Vec<Field>,
}

impl Method {
    /// Whether an agent must ask before calling it: a delete, or a change
    /// that can't be undone.
    pub fn destructive(&self) -> bool {
        self.delete || !self.one_way.is_empty()
    }
}

/// A resource, a message with `google.api.resource`.
#[derive(Debug)]
pub struct Resource {
    /// `pvs.origincloudapis.eu/Volume`.
    pub type_name: String,
    /// `Volume`.
    pub message: String,
    /// `volume`.
    pub singular: String,
    /// `volumes`.
    pub plural: String,
    /// `projects/{project}/zones/{zone}/volumes/{volume}`.
    pub patterns: Vec<String>,
    pub about: String,
    pub fields: Vec<Field>,
}

impl Resource {
    /// The full resource name of each pattern:
    /// `//pvs.origincloudapis.eu/projects/{project}/...`.
    pub fn full_names(&self) -> Vec<String> {
        let host = self.type_name.split('/').next().unwrap_or_default();
        self.patterns
            .iter()
            .map(|p| format!("//{host}/{p}"))
            .collect()
    }
}

/// A field of a resource or a request.
#[derive(Debug, Clone)]
pub struct Field {
    /// `size_gib`.
    pub name: String,
    /// `sizeGib`, its name in the API's JSON.
    pub json_name: String,
    /// `int64`, `string`, `Volume.State`, `map<string, string>`, with
    /// `repeated` before a list.
    pub type_label: String,
    pub behaviors: Vec<Behavior>,
    /// The resource type a reference field names.
    pub reference: Option<String>,
    /// The oneof it is a member of.
    pub oneof: Option<String>,
    /// The values of an enum field, without the unspecified one.
    pub values: Vec<String>,
    pub about: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Behavior {
    Identifier,
    Required,
    Optional,
    Immutable,
    OutputOnly,
    InputOnly,
}

impl Behavior {
    pub fn label(self) -> &'static str {
        match self {
            Behavior::Identifier => "identifier",
            Behavior::Required => "required",
            Behavior::Optional => "optional",
            Behavior::Immutable => "immutable",
            Behavior::OutputOnly => "output only",
            Behavior::InputOnly => "input only",
        }
    }
}

impl Field {
    pub fn has(&self, behavior: Behavior) -> bool {
        self.behaviors.contains(&behavior)
    }

    /// Its behaviors as words: `required, immutable`; `optional` when it
    /// has none of required, output only and identifier.
    pub fn behavior_label(&self) -> String {
        let mut words: Vec<&str> = self.behaviors.iter().map(|b| b.label()).collect();
        if !self.has(Behavior::Required)
            && !self.has(Behavior::OutputOnly)
            && !self.has(Behavior::Identifier)
            && !self.has(Behavior::Optional)
        {
            words.insert(0, "optional");
        }
        words.join(", ")
    }
}

/// Builds the model of the service whose newest package is `package`.
pub fn service(
    pool: &DescriptorPool,
    comments: &Comments,
    policy: &Policy,
    name: &str,
    package: &str,
) -> Service {
    let mut files: Vec<_> = pool
        .files()
        .filter(|f| f.package_name() == package)
        .collect();
    files.sort_by(|a, b| a.name().cmp(b.name()));

    let mut resources = Vec::new();
    for file in &files {
        for message in file.messages() {
            if let Some(resource) = resource(pool, comments, &message) {
                resources.push(resource);
            }
        }
    }

    let mut host = String::new();
    let mut interfaces = Vec::new();
    for file in &files {
        for s in file.services() {
            if host.is_empty() {
                host = string_extension(pool, &s.options(), "google.api.default_host")
                    .unwrap_or_default();
            }
            interfaces.push(interface(pool, comments, policy, name, &s, &resources));
        }
    }
    if host.is_empty() {
        host = format!("{name}.origincloudapis.eu");
    }
    let operations = interfaces
        .iter()
        .flat_map(|i| &i.methods)
        .any(|m| m.long_running.is_some());
    Service {
        name: name.to_owned(),
        version: package.rsplit('.').next().unwrap_or_default().to_owned(),
        host,
        interfaces,
        resources,
        operations,
    }
}

fn interface(
    pool: &DescriptorPool,
    comments: &Comments,
    policy: &Policy,
    service: &str,
    s: &ServiceDescriptor,
    resources: &[Resource],
) -> Interface {
    let about = docs::clean(comments.leading(s.parent_file().name(), s.path()));
    let methods = s
        .methods()
        .map(|m| method(pool, comments, policy, service, &m, resources))
        .collect();
    Interface {
        name: s.name().to_owned(),
        about,
        methods,
    }
}

fn method(
    pool: &DescriptorPool,
    comments: &Comments,
    policy: &Policy,
    service: &str,
    m: &MethodDescriptor,
    resources: &[Resource],
) -> Method {
    let name = m.name().to_owned();
    let kind = kind_of(&name);
    let long_running = operation_result(pool, m);
    let request = m.input();
    let fields: Vec<Field> = request
        .fields()
        .map(|f| field(pool, comments, &f))
        .collect();
    let one_way = policy
        .one_way
        .iter()
        .filter(|o| o.service == service && o.method == name)
        .map(|o| (o.field.clone(), o.consequence.clone()))
        .collect();
    let no_tool = policy
        .no_tool
        .iter()
        .find(|n| n.service == service && n.method == name)
        .map(|n| n.why.clone());
    Method {
        resource: subject(&name, kind, resources),
        full_name: m.full_name().to_owned(),
        kind,
        about: docs::clean(comments.leading(m.parent_file().name(), m.path())),
        delete: kind == Kind2::Delete,
        request: request.name().to_owned(),
        long_running: long_running.map(|r| r.name().to_owned()),
        one_way,
        no_tool,
        request_id: request.get_field_by_name("request_id").is_some(),
        items: items(m),
        fields,
        name,
    }
}

/// The repeated message field of a list's response (AIP-158): the request
/// has a `page_token`, the response a `next_page_token` and exactly one
/// repeated message field.
fn items(m: &MethodDescriptor) -> Option<(String, String)> {
    let request = m.input();
    let response = m.output();
    request.get_field_by_name("page_token")?;
    response.get_field_by_name("next_page_token")?;
    let repeated: Vec<FieldDescriptor> = response
        .fields()
        .filter(|f| f.is_list() && matches!(f.kind(), Kind::Message(_)))
        .collect();
    match repeated.as_slice() {
        [one] => Some((one.name().to_owned(), one.json_name().to_owned())),
        _ => None,
    }
}

/// The AIP standard method a name is, by its verb.
fn kind_of(name: &str) -> Kind2 {
    for (prefix, kind) in [
        ("Get", Kind2::Get),
        ("List", Kind2::List),
        ("Create", Kind2::Create),
        ("Update", Kind2::Update),
        ("Delete", Kind2::Delete),
    ] {
        if name
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_uppercase()))
        {
            return kind;
        }
    }
    Kind2::Custom
}

/// The resource a method acts on: the one its name ends with, by the
/// resource's message (`CreateVolume`, `ProtectSnapshot`) or its plural
/// (`ListVolumes`), the longest that matches.
fn subject(name: &str, kind: Kind2, resources: &[Resource]) -> Option<String> {
    use heck::ToUpperCamelCase;
    let mut best: Option<(usize, &Resource)> = None;
    for r in resources {
        let plural = r.plural.to_upper_camel_case();
        let candidates = [r.message.as_str(), plural.as_str()];
        for c in candidates {
            let matches = if kind == Kind2::Custom {
                name.ends_with(c) || name.contains(c)
            } else {
                name.ends_with(c)
            };
            if matches && best.is_none_or(|(len, _)| c.len() > len) {
                best = Some((c.len(), r));
            }
        }
    }
    best.map(|(_, r)| r.type_name.clone())
}

fn resource(
    pool: &DescriptorPool,
    comments: &Comments,
    message: &MessageDescriptor,
) -> Option<Resource> {
    let options = message_extension(pool, &message.options(), "google.api.resource")?;
    let get = |name: &str| -> String {
        options
            .get_field_by_name(name)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    let patterns = match options.get_field_by_name("pattern").map(|v| v.into_owned()) {
        Some(Value::List(values)) => values
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    Some(Resource {
        type_name: get("type"),
        message: message.name().to_owned(),
        singular: get("singular"),
        plural: get("plural"),
        patterns,
        about: docs::clean(comments.leading(message.parent_file().name(), message.path())),
        fields: message
            .fields()
            .map(|f| field(pool, comments, &f))
            .collect(),
    })
}

fn field(pool: &DescriptorPool, comments: &Comments, f: &FieldDescriptor) -> Field {
    let mut behaviors: Vec<Behavior> = behaviors(pool, f)
        .into_iter()
        .filter_map(|b| match b {
            OPTIONAL => Some(Behavior::Optional),
            REQUIRED => Some(Behavior::Required),
            OUTPUT_ONLY => Some(Behavior::OutputOnly),
            INPUT_ONLY => Some(Behavior::InputOnly),
            IMMUTABLE => Some(Behavior::Immutable),
            IDENTIFIER => Some(Behavior::Identifier),
            _ => None,
        })
        .collect();
    behaviors.sort();
    behaviors.dedup();
    let reference = message_extension(pool, &f.options(), "google.api.resource_reference")
        .and_then(|r| {
            ["type", "child_type"].iter().find_map(|k| {
                r.get_field_by_name(k)
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .filter(|s| !s.is_empty())
            })
        });
    let values = match f.kind() {
        Kind::Enum(e) => e
            .values()
            .filter(|v| v.number() != 0)
            .map(|v| v.name().to_owned())
            .collect(),
        _ => Vec::new(),
    };
    Field {
        name: f.name().to_owned(),
        json_name: f.json_name().to_owned(),
        type_label: type_label(f),
        behaviors,
        reference,
        oneof: f
            .containing_oneof()
            .filter(|o| !o.is_synthetic())
            .map(|o| o.name().to_owned()),
        values,
        about: docs::clean(comments.leading(f.parent_file().name(), f.path())),
    }
}

/// `int64`, `Volume.State`, `map<string, string>`, `repeated Rule`.
fn type_label(f: &FieldDescriptor) -> String {
    if f.is_map() {
        if let Kind::Message(entry) = f.kind() {
            let k = entry.map_entry_key_field();
            let v = entry.map_entry_value_field();
            return format!("map<{}, {}>", kind_label(&k), kind_label(&v));
        }
    }
    let label = kind_label(f);
    if f.is_list() {
        format!("repeated {label}")
    } else {
        label
    }
}

fn kind_label(f: &FieldDescriptor) -> String {
    let local = |full: &str, package: &str| -> String {
        full.strip_prefix(package)
            .and_then(|r| r.strip_prefix('.'))
            .unwrap_or(full)
            .to_owned()
    };
    let package = f.parent_file().package_name().to_owned();
    match f.kind() {
        Kind::Double => "double".into(),
        Kind::Float => "float".into(),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => "int32".into(),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => "int64".into(),
        Kind::Uint32 | Kind::Fixed32 => "uint32".into(),
        Kind::Uint64 | Kind::Fixed64 => "uint64".into(),
        Kind::Bool => "bool".into(),
        Kind::String => "string".into(),
        Kind::Bytes => "bytes".into(),
        Kind::Message(m) => match m.full_name() {
            "google.protobuf.Timestamp" => "timestamp".into(),
            "google.protobuf.Duration" => "duration".into(),
            "google.protobuf.FieldMask" => "field mask".into(),
            full => local(full, &package),
        },
        Kind::Enum(e) => local(e.full_name(), &package),
    }
}

/// The message an operation's result is, for a method answering with an
/// operation.
fn operation_result(pool: &DescriptorPool, m: &MethodDescriptor) -> Option<MessageDescriptor> {
    let info = message_extension(pool, &m.options(), "google.longrunning.operation_info")?;
    let name = info
        .get_field_by_name("response_type")?
        .as_str()?
        .to_owned();
    let name = if name.contains('.') {
        name
    } else {
        format!("{}.{name}", m.parent_file().package_name())
    };
    pool.get_message_by_name(&name)
}

fn message_extension(
    pool: &DescriptorPool,
    options: &DynamicMessage,
    name: &str,
) -> Option<DynamicMessage> {
    let ext = pool.get_extension_by_name(name)?;
    if !options.has_extension(&ext) {
        return None;
    }
    match options.get_extension(&ext).into_owned() {
        Value::Message(m) => Some(m),
        _ => None,
    }
}

fn string_extension(pool: &DescriptorPool, options: &DynamicMessage, name: &str) -> Option<String> {
    let ext = pool.get_extension_by_name(name)?;
    if !options.has_extension(&ext) {
        return None;
    }
    options.get_extension(&ext).as_str().map(str::to_owned)
}

fn behaviors(pool: &DescriptorPool, field: &FieldDescriptor) -> Vec<i32> {
    let Some(ext) = pool.get_extension_by_name("google.api.field_behavior") else {
        return Vec::new();
    };
    let options = field.options();
    if !options.has_extension(&ext) {
        return Vec::new();
    }
    match options.get_extension(&ext).into_owned() {
        Value::List(values) => values.iter().filter_map(Value::as_enum_number).collect(),
        _ => Vec::new(),
    }
}

/// Every method of the services, by full name.
pub fn methods(services: &[Service]) -> BTreeMap<&str, &Method> {
    services
        .iter()
        .flat_map(|s| &s.interfaces)
        .flat_map(|i| &i.methods)
        .map(|m| (m.full_name.as_str(), m))
        .collect()
}
