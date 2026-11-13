//! A stand-in for Origin Cloud's API, for the evals of the `origin` skill:
//! `scripts/eval.sh` runs the `origin` CLI against it instead of the
//! platform, with no credentials.
//!
//! ```sh
//! fake-api --world world.json --log requests.log --env endpoints.env
//! ```
//!
//! It serves every service of the API on a port of its own, from one world
//! of resources, and writes `ORIGINCLOUD_{SERVICE}_ENDPOINT=http://…` lines
//! to `--env` once it listens. The world (see [`World`]) holds the
//! resources by their names, canned answers for methods it doesn't model,
//! and operations already under way. Reads, lists, creates, updates,
//! deletes and other methods on a resource behave as the API's standard
//! methods do; the changes of `compute`, `network` and `pvs` answer with an
//! operation that is done a moment later, and only then take effect. Every
//! request goes to `--log` as one line: `METHOD /path?query STATUS body`.

mod world;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use fake_server::{FakeServer, Received, Response};
use tokio::signal::unix::{signal, SignalKind};

use world::{Answer, State, World};

/// The services of the API, each served on a port of its own, as the CLI
/// sends each group's calls to its own endpoint.
const SERVICES: &[&str] = &[
    "billing",
    "compute",
    "dns",
    "iam",
    "location",
    "network",
    "observability",
    "pvs",
    "resourcemanager",
    "sts",
    "support",
];

struct Args {
    world: PathBuf,
    log: PathBuf,
    env: PathBuf,
}

fn args() -> Result<Args> {
    let mut world = None;
    let mut log = None;
    let mut env = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let value = it.next().with_context(|| format!("{a} needs a value"))?;
        match a.as_str() {
            "--world" => world = Some(PathBuf::from(value)),
            "--log" => log = Some(PathBuf::from(value)),
            "--env" => env = Some(PathBuf::from(value)),
            _ => bail!("usage: fake-api --world FILE --log FILE --env FILE"),
        }
    }
    Ok(Args {
        world: world.context("--world is required")?,
        log: log.context("--log is required")?,
        env: env.context("--env is required")?,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = args()?;
    let text = std::fs::read_to_string(&args.world)
        .with_context(|| format!("reading {}", args.world.display()))?;
    let world: World =
        serde_json::from_str(&text).with_context(|| format!("reading {}", args.world.display()))?;
    let log = std::fs::File::create(&args.log)
        .with_context(|| format!("creating {}", args.log.display()))?;
    let state = Arc::new(Mutex::new(State::new(world, Box::new(log))));

    let mut servers = Vec::new();
    let mut env = String::new();
    for service in SERVICES {
        let state = Arc::clone(&state);
        let server = FakeServer::start(move |request: Received| {
            let state = Arc::clone(&state);
            async move { serve(&state, service, request).await }
        })
        .await;
        env.push_str(&format!(
            "ORIGINCLOUD_{}_ENDPOINT={}\n",
            service.to_uppercase(),
            server.url()
        ));
        servers.push(server);
    }
    // Written whole, so that a reader never sees half of it.
    let partial = args.env.with_extension("partial");
    std::fs::write(&partial, env)?;
    std::fs::rename(&partial, &args.env)?;

    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
    drop(servers);
    Ok(())
}

/// Answers one request, waiting first when it waits on an operation.
async fn serve(state: &Mutex<State>, service: &str, request: Received) -> Response {
    let line = format!(
        "{} {}{}{}",
        request.method,
        request.path,
        if request.query.is_empty() { "" } else { "?" },
        request.query
    );
    let query = request.query_pairs();
    let body = request.text();
    let mut answer =
        state
            .lock()
            .unwrap()
            .handle(service, &request.method, &request.path, &query, &body);
    if let Answer::Wait { operation, until } = answer {
        tokio::time::sleep_until(tokio::time::Instant::from_std(until)).await;
        answer = state.lock().unwrap().operation(&operation);
    }
    let (status, json) = match answer {
        Answer::Json(status, json) => (status, json),
        Answer::Wait { .. } => unreachable!("an operation is waited on once"),
    };
    let text = json.to_string();
    state
        .lock()
        .unwrap()
        .log(&format!("{line} {status} {}", body.replace('\n', " ")));
    // A moment's delay, as a real call has, so that a command's waiting
    // shows in its timing.
    tokio::time::sleep(Duration::from_millis(20)).await;
    Response::json(text).with_status(status)
}
