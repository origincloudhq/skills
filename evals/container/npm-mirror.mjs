// A package registry for an eval's container, which has no way to the
// internet: the npm packages the origincloud-sdk skill's TypeScript tasks
// use, at the versions the image pins, and what they depend on.
//
//   node npm-mirror.mjs build DIR SPEC...   # at image build time, online
//   node npm-mirror.mjs serve DIR PORT      # in the container
//
// `build` fetches each SPEC (`name@version` or `name@range`, the highest
// version matching) from registry.npmjs.org, and its dependencies, the same
// way, until nothing is missing, and writes DIR/{name}.json, a packument
// that lists only the versions fetched, its `latest` the highest, and
// DIR/tarballs/. An optional dependency for another platform (its `os` or
// `cpu` not this machine's) is left out, as npm leaves it uninstalled.
// `serve` answers npm's requests for them on 127.0.0.1:PORT, and with 404
// for any other package.

import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const semver = require("/usr/local/lib/node_modules/npm/node_modules/semver");

const UPSTREAM = "https://registry.npmjs.org";

const [mode, dir, ...rest] = process.argv.slice(2);
if (mode === "build" && dir && rest.length > 0) {
  await build(dir, rest);
} else if (mode === "serve" && dir && rest.length === 1) {
  serve(dir, Number(rest[0]));
} else {
  console.error("usage: npm-mirror.mjs build DIR SPEC... | serve DIR PORT");
  process.exit(2);
}

/** The file of a package's packument: `@scope/name` as `@scope%2fname`. */
function file(name) {
  return `${name.replace("/", "%2f")}.json`;
}

/** Whether a manifest is installable here, by its `os` and `cpu`. */
function fits(manifest) {
  const allowed = (list, value) => {
    if (!list || list.length === 0) return true;
    if (list.includes(`!${value}`)) return false;
    const only = list.filter((x) => !x.startsWith("!"));
    return only.length === 0 || only.includes(value);
  };
  return allowed(manifest.os, process.platform) && allowed(manifest.cpu, process.arch);
}

async function build(dir, specs) {
  await mkdir(join(dir, "tarballs"), { recursive: true });
  const upstream = new Map();
  const chosen = new Map(); // name -> Map(version -> manifest)
  const queue = specs.map((spec) => {
    const at = spec.lastIndexOf("@");
    return at > 0 ? [spec.slice(0, at), spec.slice(at + 1)] : [spec, "latest"];
  });
  while (queue.length > 0) {
    const [name, range] = queue.shift();
    if (!upstream.has(name)) {
      const response = await fetch(`${UPSTREAM}/${name.replace("/", "%2f")}`);
      if (!response.ok) throw new Error(`${name}: ${response.status}`);
      upstream.set(name, await response.json());
    }
    const packument = upstream.get(name);
    const version =
      packument["dist-tags"][range] ??
      semver.maxSatisfying(Object.keys(packument.versions), range);
    if (!version) throw new Error(`${name}@${range}: no such version`);
    const versions = chosen.get(name) ?? new Map();
    chosen.set(name, versions);
    if (versions.has(version)) continue;
    const manifest = packument.versions[version];
    versions.set(version, manifest);
    const optional = manifest.optionalDependencies ?? {};
    for (const [dep, depRange] of Object.entries({ ...manifest.dependencies, ...manifest.peerDependencies })) {
      if (dep in optional) continue;
      queue.push([dep, depRange]);
    }
    for (const [dep, depRange] of Object.entries(optional)) {
      const packument = await (await fetch(`${UPSTREAM}/${dep.replace("/", "%2f")}`)).json();
      const v = semver.maxSatisfying(Object.keys(packument.versions), depRange);
      if (v && fits(packument.versions[v])) {
        upstream.set(dep, packument);
        queue.push([dep, depRange]);
      }
    }
  }
  for (const [name, versions] of chosen) {
    const out = { name, "dist-tags": {}, versions: {} };
    for (const [version, manifest] of versions) {
      const tarball = manifest.dist.tarball;
      const base = tarball.slice(tarball.lastIndexOf("/") + 1);
      const bytes = Buffer.from(await (await fetch(tarball)).arrayBuffer());
      if (manifest.dist.integrity) {
        const [algorithm, digest] = manifest.dist.integrity.split("-", 2);
        if (createHash(algorithm).update(bytes).digest("base64") !== digest) {
          throw new Error(`${name}@${version}: the tarball doesn't match its integrity`);
        }
      }
      const local = `${name.replace("/", "%2f")}-${base}`;
      await writeFile(join(dir, "tarballs", local), bytes);
      out.versions[version] = {
        ...manifest,
        dist: { ...manifest.dist, tarball: `http://127.0.0.1:PORT/-/tarballs/${local}` },
      };
    }
    out["dist-tags"].latest = semver.rsort([...versions.keys()])[0];
    await writeFile(join(dir, file(name)), JSON.stringify(out));
    console.log(`${name}: ${[...versions.keys()].join(", ")}`);
  }
}

function serve(dir, port) {
  createServer(async (request, response) => {
    const path = decodeURIComponent(new URL(request.url, "http://x").pathname).slice(1);
    try {
      if (path.startsWith("-/tarballs/")) {
        const local = path.slice("-/tarballs/".length).replace("/", "%2f");
        const bytes = await readFile(join(dir, "tarballs", local));
        response.writeHead(200, { "content-type": "application/octet-stream" });
        response.end(bytes);
      } else {
        const text = await readFile(join(dir, file(path)), "utf8");
        response.writeHead(200, { "content-type": "application/json" });
        response.end(text.replaceAll("http://127.0.0.1:PORT/", `http://127.0.0.1:${port}/`));
      }
    } catch {
      response.writeHead(404, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: `${path} isn't in this eval's registry` }));
    }
  }).listen(port, "127.0.0.1");
}
