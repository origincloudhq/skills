//! The fake API's world: its resources and operations, and how a request
//! changes them.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Map, Value};

/// The services whose changes answer with an operation.
const OPERATION_SERVICES: &[&str] = &["compute", "network", "pvs"];

/// How long a change's operation takes.
const SETTLE: Duration = Duration::from_millis(1500);

/// The longest a `:wait` call waits when it names no timeout.
const WAIT: Duration = Duration::from_secs(60);

/// What a world file holds.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct World {
    /// The resources there are, each with its `name`.
    pub resources: Vec<Value>,
    /// Answers for methods the fake doesn't model, matched first.
    pub responses: Vec<Canned>,
    /// Operations under way when the fake starts.
    pub operations: Vec<Pending>,
    /// Fields a created resource gets, by its collection, such as
    /// `{"floatingIps": {"address": "192.0.2.10"}}`.
    pub created: BTreeMap<String, Map<String, Value>>,
    /// The prefix of a created resource's id, by its collection, such as
    /// `{"volumes": "vol"}`; `res` otherwise.
    pub ids: BTreeMap<String, String>,
    /// The zone of the operations of resources that aren't in a zone.
    pub zone: Option<String>,
    /// The CLI profile the evals write for the world; not read here.
    pub profile: Option<Value>,
}

/// A canned answer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canned {
    pub method: String,
    /// A regular expression the whole path, such as
    /// `/v1beta1/projects/p:queryCost`, matches.
    pub path: String,
    #[serde(default = "ok")]
    pub status: u16,
    pub body: Value,
}

fn ok() -> u16 {
    200
}

/// An operation under way when the fake starts.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    /// The operation's name.
    pub name: String,
    /// When it is done, in seconds after the fake started.
    pub done_after_secs: f64,
    /// Its result once done, with its `@type`.
    #[serde(default)]
    pub response: Option<Value>,
    /// The resources it creates or changes once done.
    #[serde(default)]
    pub upsert: Vec<Value>,
}

/// What a request is answered with.
pub enum Answer {
    Json(u16, Value),
    /// Wait until `until`, then answer with the operation as it is then.
    Wait {
        operation: String,
        until: Instant,
    },
}

enum Effect {
    Upsert(Vec<Value>),
    Remove(String),
}

struct Operation {
    json: Value,
    done_at: Instant,
    effect: Option<Effect>,
}

pub struct State {
    resources: BTreeMap<String, Value>,
    operations: BTreeMap<String, Operation>,
    canned: Vec<(String, Regex, u16, Value)>,
    created: BTreeMap<String, Map<String, Value>>,
    ids: BTreeMap<String, String>,
    zone: String,
    /// The operation each `requestId` started, for retries.
    requests: BTreeMap<String, Value>,
    counter: u64,
    log: Box<dyn Write + Send>,
}

impl State {
    pub fn new(world: World, log: Box<dyn Write + Send>) -> Self {
        let now = Instant::now();
        let resources = world
            .resources
            .into_iter()
            .filter_map(|r| Some((r.get("name")?.as_str()?.to_owned(), r)))
            .collect();
        let operations = world
            .operations
            .into_iter()
            .map(|p| {
                let mut json = json!({ "name": p.name, "done": false });
                if let Some(response) = p.response {
                    json["pendingResponse"] = response;
                }
                (
                    p.name,
                    Operation {
                        json,
                        done_at: now + Duration::from_secs_f64(p.done_after_secs),
                        effect: Some(Effect::Upsert(p.upsert)),
                    },
                )
            })
            .collect();
        let canned = world
            .responses
            .into_iter()
            .map(|c| {
                let re = Regex::new(&format!("^(?:{})$", c.path)).expect("a valid path pattern");
                (c.method, re, c.status, c.body)
            })
            .collect();
        Self {
            resources,
            operations,
            canned,
            created: world.created,
            ids: world.ids,
            zone: world.zone.unwrap_or_else(|| "eu-central1-az1".to_owned()),
            requests: BTreeMap::new(),
            counter: 0,
            log,
        }
    }

    pub fn log(&mut self, line: &str) {
        let _ = writeln!(self.log, "{line}");
        let _ = self.log.flush();
    }

    /// Answers a request to `service`.
    pub fn handle(
        &mut self,
        service: &str,
        method: &str,
        path: &str,
        query: &[(String, String)],
        body: &str,
    ) -> Answer {
        self.settle(Instant::now());
        let path = decode(path);
        for (m, re, status, answer) in &self.canned {
            if m.eq_ignore_ascii_case(method) && re.is_match(&path) {
                return Answer::Json(*status, answer.clone());
            }
        }
        let Some((version, rest)) = path
            .strip_prefix('/')
            .and_then(|p| p.split_once('/'))
            .filter(|(v, _)| v.starts_with('v'))
        else {
            return error(404, "NOT_FOUND", &format!("no method at {path}"));
        };
        let body: Value = if body.trim().is_empty() {
            json!({})
        } else {
            match serde_json::from_str(body) {
                Ok(b) => b,
                Err(e) => return error(400, "BAD_REQUEST", &format!("the body isn't JSON: {e}")),
            }
        };
        let request_id = query
            .iter()
            .find(|(k, _)| k == "requestId")
            .map(|(_, v)| v.clone());
        if let Some(previous) = request_id.as_ref().and_then(|id| self.requests.get(id)) {
            return Answer::Json(200, previous.clone());
        }
        let lro = OPERATION_SERVICES.contains(&service);

        // A custom method, `{name}:{verb}`.
        if let Some((target, verb)) = rest.rsplit_once(':').filter(|(_, v)| !v.contains('/')) {
            return self.custom(service, version, target, verb, &body, lro);
        }
        let segments = rest.split('/').count();
        let answer = match (method, segments % 2) {
            ("GET", 0) => self.get(rest),
            ("GET", _) => self.list(rest, query),
            ("POST", 1) => self.create(service, version, rest, query, body, lro),
            ("PATCH", 0) => self.update(service, version, rest, &body, lro),
            ("DELETE", 0) => self.delete(rest, lro),
            _ => error(405, "METHOD_NOT_ALLOWED", &format!("{method} {path}")),
        };
        if let (Some(id), Answer::Json(200, json)) = (request_id, &answer) {
            self.requests.insert(id, json.clone());
        }
        answer
    }

    /// The operation `name` as it is now.
    pub fn operation(&mut self, name: &str) -> Answer {
        self.settle(Instant::now());
        match self.operations.get(name) {
            Some(op) => Answer::Json(200, public(&op.json)),
            None => error(404, "NOT_FOUND", &format!("{name} not found")),
        }
    }

    /// Finishes the operations whose time has come.
    fn settle(&mut self, now: Instant) {
        let due: Vec<String> = self
            .operations
            .iter()
            .filter(|(_, op)| op.json["done"] != true && op.done_at <= now)
            .map(|(name, _)| name.clone())
            .collect();
        for name in due {
            let op = self.operations.get_mut(&name).expect("a due operation");
            op.json["done"] = Value::Bool(true);
            if let Some(response) = op
                .json
                .as_object_mut()
                .and_then(|o| o.remove("pendingResponse"))
            {
                op.json["response"] = response;
            }
            match op.effect.take() {
                Some(Effect::Upsert(resources)) => {
                    for r in resources {
                        if let Some(n) = r.get("name").and_then(Value::as_str) {
                            let n = n.to_owned();
                            self.resources.insert(n, r);
                        }
                    }
                }
                Some(Effect::Remove(n)) => {
                    self.resources.remove(&n);
                }
                None => {}
            }
        }
    }

    fn get(&mut self, name: &str) -> Answer {
        if self.operations.contains_key(name) {
            return self.operation(name);
        }
        match self.resources.get(name) {
            Some(r) => Answer::Json(200, r.clone()),
            None => error(404, "NOT_FOUND", &format!("{name} not found")),
        }
    }

    /// The resources in a collection, `-` in the parent matching any id;
    /// a query parameter that names one of their fields keeps those whose
    /// field has that value.
    fn list(&self, collection: &str, query: &[(String, String)]) -> Answer {
        let pattern: Vec<&str> = collection.split('/').collect();
        let key = list_field(collection);
        let filters: Vec<&(String, String)> = query
            .iter()
            .filter(|(k, v)| !v.is_empty() && !matches!(k.as_str(), "pageSize" | "pageToken"))
            .collect();
        let items: Vec<Value> = self
            .resources
            .iter()
            .filter(|(name, _)| {
                let segments: Vec<&str> = name.split('/').collect();
                segments.len() == pattern.len() + 1
                    && pattern
                        .iter()
                        .zip(&segments)
                        .all(|(p, s)| *p == "-" || p == s)
            })
            .filter(|(_, r)| {
                filters.iter().all(|(k, v)| match r.get(k) {
                    Some(Value::String(s)) => s == v || s.rsplit('/').next() == Some(v),
                    _ => true,
                })
            })
            .map(|(_, r)| r.clone())
            .collect();
        Answer::Json(200, json!({ key: items }))
    }

    fn create(
        &mut self,
        service: &str,
        version: &str,
        collection: &str,
        query: &[(String, String)],
        body: Value,
        lro: bool,
    ) -> Answer {
        let key = collection.rsplit('/').next().unwrap_or_default().to_owned();
        // The parent, such as the instance of a volume attachment, must be
        // there; a project, zone or region is taken as it is named.
        if let Some((parent, _)) = collection.rsplit_once('/') {
            let parent_collection = parent.rsplit('/').nth(1).unwrap_or_default();
            if !matches!(
                parent_collection,
                "" | "projects" | "zones" | "regions" | "organizations"
            ) && !self.resources.contains_key(parent)
            {
                return error(404, "NOT_FOUND", &format!("{parent} not found"));
            }
        }
        if let Err(answer) = self.check_references(&body) {
            return answer;
        }
        self.counter += 1;
        let id = query
            .iter()
            .find(|(k, v)| k.ends_with("Id") && k != "requestId" && !v.is_empty())
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| {
                let prefix = self.ids.get(&key).map(String::as_str).unwrap_or("res");
                format!("{prefix}-{:04x}", 0x1a2b + self.counter)
            });
        let name = format!("{collection}/{id}");
        if self.resources.contains_key(&name) {
            return error(409, "ALREADY_EXISTS", &format!("{name} already exists"));
        }
        let mut resource = match body {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        if let Some(defaults) = self.created.get(&key) {
            for (k, v) in defaults {
                resource.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        resource.insert("name".to_owned(), Value::String(name.clone()));
        let resource = Value::Object(resource);
        if lro {
            let response = typed(service, version, &name, &resource);
            self.start(&name, response, Effect::Upsert(vec![resource]))
        } else {
            self.resources.insert(name, resource.clone());
            Answer::Json(200, resource)
        }
    }

    fn update(
        &mut self,
        service: &str,
        version: &str,
        name: &str,
        body: &Value,
        lro: bool,
    ) -> Answer {
        let Some(current) = self.resources.get(name) else {
            return error(404, "NOT_FOUND", &format!("{name} not found"));
        };
        let mut changed = current.clone();
        if let (Some(target), Some(fields)) = (changed.as_object_mut(), body.as_object()) {
            for (k, v) in fields {
                if k != "name" {
                    target.insert(k.clone(), v.clone());
                }
            }
        }
        if lro {
            let response = typed(service, version, name, &changed);
            self.start(name, response, Effect::Upsert(vec![changed]))
        } else {
            self.resources.insert(name.to_owned(), changed.clone());
            Answer::Json(200, changed)
        }
    }

    fn delete(&mut self, name: &str, lro: bool) -> Answer {
        if !self.resources.contains_key(name) {
            return error(404, "NOT_FOUND", &format!("{name} not found"));
        }
        if lro {
            let response = json!({ "@type": "type.googleapis.com/google.protobuf.Empty" });
            self.start(name, response, Effect::Remove(name.to_owned()))
        } else {
            self.resources.remove(name);
            Answer::Json(200, json!({}))
        }
    }

    fn custom(
        &mut self,
        service: &str,
        version: &str,
        target: &str,
        verb: &str,
        body: &Value,
        lro: bool,
    ) -> Answer {
        if verb == "wait" {
            let Some(op) = self.operations.get(target) else {
                return error(404, "NOT_FOUND", &format!("{target} not found"));
            };
            let timeout = body
                .get("timeout")
                .and_then(Value::as_str)
                .and_then(|t| t.strip_suffix('s'))
                .and_then(|t| t.parse::<f64>().ok())
                .map(Duration::from_secs_f64)
                .unwrap_or(WAIT);
            let until = op.done_at.min(Instant::now() + timeout);
            return Answer::Wait {
                operation: target.to_owned(),
                until,
            };
        }
        let Some(current) = self.resources.get(target) else {
            return error(404, "NOT_FOUND", &format!("{target} not found"));
        };
        let mut changed = current.clone();
        match verb {
            "start" => changed["state"] = json!("RUNNING"),
            "stop" => changed["state"] = json!("STOPPED"),
            "protect" => changed["protectionState"] = json!("PROTECTED"),
            "unprotect" => changed["protectionState"] = json!("UNPROTECTED"),
            _ => {}
        }
        if lro {
            let response = typed(service, version, target, &changed);
            self.start(target, response, Effect::Upsert(vec![changed]))
        } else {
            Answer::Json(200, changed)
        }
    }

    /// A create naming a resource that isn't there, or isn't ready, is
    /// refused, as the API refuses it; so is a role binding for a member
    /// no account has.
    fn check_references(&self, body: &Value) -> Result<(), Answer> {
        let name = Regex::new(
            r"^(projects|organizations|zones|regions|roles|instanceTypes|imageFamilies|storageClasses)/[^/]+(/|$)",
        )
        .expect("a valid pattern");
        if let Some(member) = body.get("member").and_then(Value::as_str) {
            if !self
                .resources
                .values()
                .any(|r| r.get("member").and_then(Value::as_str) == Some(member))
            {
                return Err(error(404, "NOT_FOUND", &format!("no account is {member}")));
            }
        }
        let mut strings = Vec::new();
        collect_strings(body, &mut strings);
        for s in strings {
            if !name.is_match(s) || s.split('/').count() % 2 == 1 {
                continue;
            }
            match self.resources.get(s) {
                None => return Err(error(404, "NOT_FOUND", &format!("{s} not found"))),
                Some(r) => {
                    let state = r.get("state").and_then(Value::as_str).unwrap_or_default();
                    if matches!(state, "CREATING" | "PENDING" | "INITIALIZING") {
                        return Err(error(
                            400,
                            "FAILED_PRECONDITION",
                            &format!("{s} is {state}: wait until it is ready"),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Starts an operation on `resource` that is done after a moment.
    fn start(&mut self, resource: &str, response: Value, effect: Effect) -> Answer {
        self.counter += 1;
        let segment = |key: &str| {
            let parts: Vec<&str> = resource.split('/').collect();
            parts
                .windows(2)
                .find(|w| w[0] == key)
                .map(|w| w[1].to_owned())
        };
        let project = segment("projects").unwrap_or_else(|| "-".to_owned());
        let zone = segment("zones").unwrap_or_else(|| self.zone.clone());
        let name = format!(
            "projects/{project}/zones/{zone}/operations/op-{:04x}",
            0x3c4d + self.counter
        );
        let json = json!({ "name": name, "done": false, "pendingResponse": response });
        self.operations.insert(
            name.clone(),
            Operation {
                json: json.clone(),
                done_at: Instant::now() + SETTLE,
                effect: Some(effect),
            },
        );
        Answer::Json(200, public(&json))
    }
}

/// An operation as the API shows it: its result only once it is done.
fn public(op: &Value) -> Value {
    let mut op = op.clone();
    if let Some(o) = op.as_object_mut() {
        o.remove("pendingResponse");
    }
    op
}

fn collect_strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|v| collect_strings(v, out)),
        Value::Object(o) => o.values().for_each(|v| collect_strings(v, out)),
        _ => {}
    }
}

/// The field of a list's response that holds the resources of
/// `collection`: the collection's name, unless its message is named for
/// its parent too, as the rules of a security group, `securityGroupRules`.
fn list_field(collection: &str) -> String {
    let mut segments = collection.rsplit('/').step_by(2);
    let own = segments.next().unwrap_or_default();
    match message_named_for_parent(segments.next().unwrap_or_default(), own) {
        Some(message) => {
            let mut chars = message.chars();
            let first = chars.next().map(|c| c.to_ascii_lowercase());
            format!("{}{}s", first.unwrap_or_default(), chars.as_str())
        }
        None => own.to_owned(),
    }
}

/// The message of a collection whose name doesn't say it on its own.
fn message_named_for_parent(parent: &str, collection: &str) -> Option<&'static str> {
    match (parent, collection) {
        ("securityGroups", "rules") => Some("SecurityGroupRule"),
        ("acls", "rules") => Some("AclRule"),
        ("floatingIps", "associations") => Some("FloatingIpAssociation"),
        _ => None,
    }
}

/// `resource`, named `name`, with the `@type` of its message, as an
/// operation's result carries it: a volume is an
/// `origincloud.pvs.v1beta1.Volume`.
fn typed(service: &str, version: &str, name: &str, resource: &Value) -> Value {
    let mut collections = name.rsplit('/').skip(1).step_by(2);
    let collection = collections.next().unwrap_or_default();
    let parent = collections.next().unwrap_or_default();
    let message = match message_named_for_parent(parent, collection) {
        Some(message) => message.to_owned(),
        None => {
            let singular = if let Some(stem) = collection.strip_suffix("ies") {
                format!("{stem}y")
            } else if let Some(stem) = collection.strip_suffix("sses") {
                format!("{stem}ss")
            } else {
                collection
                    .strip_suffix('s')
                    .unwrap_or(collection)
                    .to_owned()
            };
            let mut chars = singular.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    };
    let mut typed = Map::new();
    typed.insert(
        "@type".to_owned(),
        Value::String(format!(
            "type.googleapis.com/origincloud.{service}.{version}.{message}"
        )),
    );
    if let Some(fields) = resource.as_object() {
        typed.extend(fields.clone());
    }
    Value::Object(typed)
}

/// An error as the API sends it, with its reason and a request id.
fn error(status: u16, reason: &str, message: &str) -> Answer {
    let grpc = match status {
        400 if reason == "FAILED_PRECONDITION" => "FAILED_PRECONDITION",
        400 => "INVALID_ARGUMENT",
        404 => "NOT_FOUND",
        409 => "ALREADY_EXISTS",
        _ => "UNKNOWN",
    };
    Answer::Json(
        status,
        json!({ "error": {
            "code": status,
            "status": grpc,
            "message": message,
            "details": [
                { "@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": reason, "domain": "origincloudapis.eu" },
                { "@type": "type.googleapis.com/google.rpc.RequestInfo", "requestId": "fake-0000" }
            ]
        }}),
    )
}

/// Decodes the `%XX` escapes of a path.
fn decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&path[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(world: Value) -> State {
        State::new(
            serde_json::from_value(world).unwrap(),
            Box::new(std::io::sink()),
        )
    }

    fn json(answer: Answer) -> (u16, Value) {
        match answer {
            Answer::Json(status, json) => (status, json),
            Answer::Wait { .. } => panic!("a wait"),
        }
    }

    const VOLUME: &str = "projects/p/zones/z/volumes/vol-1";

    #[test]
    fn reads_and_lists() {
        let mut s = state(json!({ "resources": [
            { "name": VOLUME, "sizeGib": 10 },
            { "name": "projects/p/zones/z/volumes/vol-2", "sizeGib": 20 },
            { "name": "projects/p/zones/y/volumes/vol-3", "sizeGib": 30 },
        ]}));
        let (status, v) = json(s.handle("pvs", "GET", &format!("/v1beta1/{VOLUME}"), &[], ""));
        assert_eq!((status, v["sizeGib"].as_i64()), (200, Some(10)));
        let (_, list) =
            json(s.handle("pvs", "GET", "/v1beta1/projects/p/zones/z/volumes", &[], ""));
        assert_eq!(list["volumes"].as_array().unwrap().len(), 2);
        let (_, all) = json(s.handle("pvs", "GET", "/v1beta1/projects/p/zones/-/volumes", &[], ""));
        assert_eq!(all["volumes"].as_array().unwrap().len(), 3);
        let (status, _) = json(s.handle(
            "pvs",
            "GET",
            "/v1beta1/projects/p/zones/z/volumes/nope",
            &[],
            "",
        ));
        assert_eq!(status, 404);
    }

    #[test]
    fn a_collection_named_for_its_parent_lists_and_types_by_its_message() {
        let rule = "projects/p/regions/r/securityGroups/sg/rules/rule-1";
        let mut s = state(json!({ "resources": [{ "name": rule, "protocol": "TCP" }] }));
        let (_, list) = json(s.handle(
            "network",
            "GET",
            "/v1beta1/projects/p/regions/r/securityGroups/sg/rules",
            &[],
            "",
        ));
        assert_eq!(list["securityGroupRules"][0]["name"], rule);
        let typed = typed("network", "v1beta1", rule, &json!({}));
        assert_eq!(
            typed["@type"],
            "type.googleapis.com/origincloud.network.v1beta1.SecurityGroupRule"
        );
        assert_eq!(list_field("projects/p/zones/z/volumes"), "volumes");
    }

    #[test]
    fn a_change_takes_effect_when_its_operation_is_done() {
        let mut s = state(json!({ "resources": [{ "name": VOLUME, "sizeGib": 10 }] }));
        let (_, op) = json(s.handle(
            "pvs",
            "PATCH",
            &format!("/v1beta1/{VOLUME}"),
            &[],
            r#"{"sizeGib":20}"#,
        ));
        assert_eq!(op["done"], false);
        let (_, v) = json(s.handle("pvs", "GET", &format!("/v1beta1/{VOLUME}"), &[], ""));
        assert_eq!(v["sizeGib"], 10);
        s.settle(Instant::now() + SETTLE);
        let (_, v) = json(s.handle("pvs", "GET", &format!("/v1beta1/{VOLUME}"), &[], ""));
        assert_eq!(v["sizeGib"], 20);
        let name = op["name"].as_str().unwrap();
        let (_, op) = json(s.operation(name));
        assert_eq!(op["done"], true);
        assert_eq!(
            op["response"]["@type"],
            "type.googleapis.com/origincloud.pvs.v1beta1.Volume"
        );
    }

    #[test]
    fn a_create_naming_a_resource_not_ready_is_refused() {
        let mut s = state(json!({ "resources": [
            { "name": VOLUME, "state": "CREATING" },
            { "name": "projects/p/zones/z/instances/i-1", "state": "RUNNING" },
        ]}));
        let (status, e) = json(s.handle(
            "compute",
            "POST",
            "/v1beta1/projects/p/zones/z/instances/i-1/volumeAttachments",
            &[],
            &format!(r#"{{"volume":"{VOLUME}"}}"#),
        ));
        assert_eq!(status, 400);
        assert_eq!(e["error"]["details"][0]["reason"], "FAILED_PRECONDITION");
    }

    #[test]
    fn a_create_under_a_parent_that_isnt_there_is_refused() {
        let mut s = state(json!({ "resources": [{ "name": VOLUME, "state": "AVAILABLE" }] }));
        let (status, _) = json(s.handle(
            "compute",
            "POST",
            "/v1beta1/projects/p/zones/z/instances/web-1/volumeAttachments",
            &[],
            &format!(r#"{{"volume":"{VOLUME}"}}"#),
        ));
        assert_eq!(status, 404);
    }

    #[test]
    fn a_create_without_operation_is_there_at_once() {
        let mut s = state(json!({ "resources": [
            { "name": "organizations/o/serviceAccounts/sa-1", "member": "serviceAccount:sa-1" },
            { "name": "roles/viewer" },
        ]}));
        let (status, _) = json(s.handle(
            "iam",
            "POST",
            "/v1beta1/organizations/o/roleBindings",
            &[],
            r#"{"member":"serviceAccount:ci","role":"roles/viewer"}"#,
        ));
        assert_eq!(status, 404, "no account is serviceAccount:ci");
        let (status, b) = json(s.handle(
            "iam",
            "POST",
            "/v1beta1/organizations/o/roleBindings",
            &[("requestId".to_owned(), "r-1".to_owned())],
            r#"{"member":"serviceAccount:sa-1","role":"roles/viewer"}"#,
        ));
        assert_eq!(status, 200);
        let name = b["name"].as_str().unwrap().to_owned();
        let (_, list) = json(s.handle(
            "iam",
            "GET",
            "/v1beta1/organizations/o/roleBindings",
            &[],
            "",
        ));
        assert_eq!(list["roleBindings"][0]["name"], name.as_str());
        // A retry with the same request id answers as the first time.
        let (_, again) = json(s.handle(
            "iam",
            "POST",
            "/v1beta1/organizations/o/roleBindings",
            &[("requestId".to_owned(), "r-1".to_owned())],
            r#"{"member":"serviceAccount:sa-1","role":"roles/viewer"}"#,
        ));
        assert_eq!(again["name"], name.as_str());
    }

    #[test]
    fn canned_answers_come_first() {
        let mut s = state(json!({ "responses": [
            { "method": "GET", "path": "/v1beta1/projects/p:queryCost", "body": { "total": "1.00" } }
        ]}));
        let (status, c) =
            json(s.handle("billing", "GET", "/v1beta1/projects/p:queryCost", &[], ""));
        assert_eq!((status, c["total"].as_str()), (200, Some("1.00")));
    }
}
