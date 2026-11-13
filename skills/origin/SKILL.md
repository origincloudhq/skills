---
name: origin
description: >-
  Use Origin Cloud (origincloud.eu) through its `origin` command-line
  interface: read and change instances, volumes and snapshots, VPCs,
  subnets, security groups and their rules, floating IPs, internet and NAT
  gateways, images and key pairs, DNS zones and records, service accounts
  and role bindings, projects, costs and usage, and support cases. Use
  whenever a task runs `origin`, names an Origin Cloud resource
  (`projects/…/zones/…`) or asks to launch, resize, open, grant, delete or
  check something on Origin Cloud; also for `origin id login` and the
  `origin mcp serve` tools.
license: Apache-2.0
metadata:
  api-revision: "227636fb9bf7bc1081c983acff81ad5b0ac70d44"
  cli-version: "1.19.0"
---
# Origin Cloud with the `origin` CLI

This skill describes the `origin` release in `metadata.cli-version`. Every
command and flag here and in the references is one that release has;
`origin <command> --help` says the rest.

## Start here

1. `origin --version`. Without `origin`, ask the user to install it from
   packages.origincloud.eu, `curl -fsSL https://packages.origincloud.eu/install.sh | sh`,
   or with apt as `origincloud-cli`.
2. `origin config profile --output json`: the default project
   (`default_project`), zone (`default_zone`) and region the commands use.
   `ORIGINCLOUD_PROJECT`, `ORIGINCLOUD_ZONE` and `ORIGINCLOUD_REGION`
   override them, and `--project`, `--zone` and `--region` override both;
   the global `--config <FILE>`, given before the command group, reads
   another profile file when the user names one. With no project set and none named by the user, ask; list the choices
   with `origin resourcemanager list-projects --organization <organization> --output json`.
3. `origin iam get-user --user me --output json` shows who is logged in.
   When a command answers `not logged in`, the user logs in, not you:
   `origin id login` opens their browser, and `origin id login --device`
   prints a code to approve on another device, for a machine without a
   browser. A CI job runs as a service account without a secret: on GitHub
   Actions with `id-token: write` every command does it on its own;
   elsewhere `origin id login --oidc-token-file <path>` with the job's OIDC
   token.

## Rules

<!-- BEGIN GENERATED rules -->
These hold on every surface: the `origin` CLI, Terraform, the SDKs and the
MCP tools.

### Credentials

- Work as the login the user already has: the `origin` CLI's session and
  profiles, or the credentials the environment holds (`ORIGINCLOUD_*`
  variables, a CI job's identity). Use the profile the user names; don't
  switch profiles or log in as someone else on your own.
- Never print, log, echo or write a token, client secret or private key:
  not in a reply, a command's output you show, a file, a commit or a
  configuration file. Don't ask the user to paste one into the
  conversation. A configuration you write names a profile or an
  environment variable, never a credential.
- A call that answers with a credential (a service account's secret, a
  generated private key) is the user's to run: tell them the command and
  where its output should go, and don't read the output yourself.

### Read before you change

- Before changing or deleting a resource, read it as it is now and base the
  change on that, not on what you expect it to be.
- Work in the project, zone and region the user works in: read the current
  context or ask. Don't guess one, and don't fall back to another.

### Ask before deletes and one-way changes

- A delete, and a change that can't be undone (growing a volume, which
  never shrinks back, is one), needs the user's yes for that resource in
  this conversation. Say what will change and what is lost, then wait.
- A yes covers what it named, nothing more. Never delete, replace or
  recreate something to get past an error.

### Wait on operations

- A create, update or delete answers with an operation. The change is done
  when the operation is `done`: then it holds the result (`response`) or
  the `error`. Wait for it before using the result, starting what depends
  on it, or telling the user it worked.
- A retried create or change carries the same `request_id` as the first
  attempt, so that it can't happen twice.

### Names

- A resource is identified by its full resource name: its service's host
  and its relative name, such as
  `//pvs.origincloudapis.eu/projects/my-project/zones/eu-central1-az1/volumes/vol-1a2b3c`.
  The resource's `name` field is the relative part,
  `projects/my-project/zones/eu-central1-az1/volumes/vol-1a2b3c`.
- A reference to another resource, an IAM binding's member and an audit
  entry's subject are full names: give the whole name, never a bare id, an
  email address or a display name.
- Take names from what the API answered; don't build them from display
  names, and don't expect a URN.

### Errors

- Report a failure as the API sent it: its code (the `reason`, such as
  `NOT_FOUND` or `CONFLICT`), its message, and the correlation ID (the
  `requestId` of its `RequestInfo`), so that support can find the call.
  Don't paraphrase it into something vaguer.
- Don't repeat a refused call unchanged. Retry only what the error says is
  retryable, with the same `request_id`; when unsure whether a change took
  effect, read the resource first.
<!-- END GENERATED rules -->

## The CLI

**JSON, always.** Add `--output json` (`-o json`) to every command whose
output you read. Without it the output is a table on a terminal, and the
user's profile can ask for a table through a pipe too. A resource prints as
the API sends it, fields in camel case (`sizeGib`); a list prints the API's
list, its items under the collection's field, every page and, without
`--zone`, every zone:

```sh
origin pvs list-volumes --output json \
  | jq -r '.volumes[] | select(.sizeGib > 40) | [.name, .sizeGib, .state] | @tsv'
origin network list-security-groups --output json \
  | jq -r '.securityGroups[] | select(.tags["origin:name"] == "web") | .name'
```

The `origin:name` tag is the name the console shows: find a resource the
user names that way by its tag, then use its `name`. `--output name`
prints only the names, one per line.

**Names.** A resource's `name` is its relative name, such as
`projects/my-project/zones/eu-central1-az1/volumes/vol-1a2b3c`; its full
name puts its service's host in front,
`//pvs.origincloudapis.eu/projects/my-project/zones/eu-central1-az1/volumes/vol-1a2b3c`,
and is how you write it to the user or across services. A flag that names
a resource takes its `name` as a command printed it, or its id (the last
segment), which the command completes with `--project` and `--zone` or
`--region` and their defaults; the CLI doesn't take the `//…` form. Pass
the whole `name` for anything outside the default project and zone. A
flag whose help says "the `{…}` part of the resource names the command
builds" takes the id alone: the organization, and the parent of what the
command lists or makes, such as `--security-group` for a rule or
`--instance` for a volume attachment (`${name##*/}` is the id). A role
binding's member is `user:<email>` or a service account's `member` field,
`serviceAccount:<id>`; read it, don't build it. Ignore `urn` fields.

**Operations.** A create, update or delete in `compute`, `network` and
`pvs` starts an operation. The command waits for it and prints the result,
or exits 1 with the operation's error, so the next command can use what it
made. With `--no-wait` it prints the operation at once; then wait for it
before using the resource:

```sh
op="$(origin pvs create-volume --size-gib 20 --no-wait --output json | jq -r .name)"
origin pvs wait-operation --operation "$op" --timeout 10m --output json
```

`origin <service> get-operation` reads one without waiting. An operation
the user started is waited on the same way, with the service that started
it. A resource that is `CREATING` isn't usable yet: wait for its operation
before the step that needs it. Every change takes
`--request-id`: generate a UUID once (`uuidgen`, or
`cat /proc/sys/kernel/random/uuid`) and pass it on the first
try and on every retry of the same change, so that it happens once.

**Deletes and one-way changes.** They refuse without `--yes` and print what
they would do; the references list them per service under "Ask before".
The refusal's re-run line is for the user. Before asking:

1. Read the resource with its `get-` command, such as
   `origin pvs get-volume --volume <volume> --output json`.
2. Find what holds or uses it: the instance a volume is attached to
   (`origin compute list-volume-attachments --instance - --zone <zone> --volume <volume> --output json`),
   the interfaces in a security group; the service's workflows say how.
3. Tell the user the resource by its `name` and its `origin:name`, what
   holds it, what changes and what is lost, and wait.

Pass `--yes` only after they said yes to that resource in this
conversation.

**Errors.** A failed command exits 1 and prints, with `--output json`,
`{"code": "failure", "description", "reason", "requestId"}`: report the
`reason` and the `requestId`. Exit 2 means the command line was wrong;
read `--help`. The codes and what to do about them:
<https://docs.origincloud.eu/docs/platform/reference/errors/index.html.md>.

**Credentials.** Don't run `origin id token` or print
`ORIGINCLOUD_CLIENT_SECRET`, `ORIGINCLOUD_ACCESS_TOKEN` or a key pair's
`privateKey`. A user who needs a token for their own tool runs
`origin id token --as-header --raw` themselves, inline:
`curl -H "$(origin id token --as-header --raw)" …`. Three commands answer
with a secret, so they are the user's to run: creating an `EXTERNAL`
service account (`origin iam create-service-account`), rotating its secret
(`origin iam rotate-service-account-secret`), and generating a key pair
(`origin compute create-key-pair` without `--public-key`).

## MCP tools

When the agent has Origin Cloud's MCP tools, served by `origin mcp serve`,
they cover the same API, one tool per command (`pvs_get_volume`,
`current_context`, …), under the same rules: the destructive ones are the
commands that need `--yes`. Use either; the CLI has every command.

## Docs

Every page of docs.origincloud.eu has a Markdown version at
`…/index.html.md`, listed in <https://docs.origincloud.eu/llms.txt>; each
reference below links its service's guide.

## References

One file per service: its commands with their flags, what to ask before,
and the API's resources with their full names and fields. Where a service
has workflows, read them before a task they cover: they hold the steps
and the checks the commands alone don't show.

<!-- BEGIN GENERATED references -->
| Service | Reference | Resources |
|---|---|---|
| billing | [references/billing.md](references/billing.md), [workflows](references/billing-workflows.md) | billingAccounts, projectBillingInfos, billingCountries, spendLimits |
| compute | [references/compute.md](references/compute.md), [workflows](references/compute-workflows.md) | instanceTypes, imageFamilies, images, instances, keyPairs, publicImages, volumeAttachments |
| dns | [references/dns.md](references/dns.md) | hostedZones, recordSets, vpcAttachments |
| iam | [references/iam.md](references/iam.md), [workflows](references/iam-workflows.md) | auditLogEntries, federatedCredentials, permissions, roles, roleBindings, serviceAccounts, users |
| location | [references/location.md](references/location.md) | regions, zones |
| network | [references/network.md](references/network.md), [workflows](references/network-workflows.md) | acls, aclAssociations, aclRules, dnsPolicies, dnsPolicyAssociations, floatingIps, floatingIpAssociations, internetGateways, natGateways, networkInterfaces, routes, routeTables, routeTableAssociations, securityGroups, securityGroupAssociations, securityGroupRules, subnets, vpcs |
| observability | [references/observability.md](references/observability.md) | — |
| pvs | [references/pvs.md](references/pvs.md), [workflows](references/pvs-workflows.md) | snapshots, storageClasses, volumes |
| resourcemanager | [references/resourcemanager.md](references/resourcemanager.md) | organizations, projects, organizationQuotas, projectQuotas |
| sts | [references/sts.md](references/sts.md) | — |
| support | [references/support.md](references/support.md) | cases, caseMessages |
<!-- END GENERATED references -->
