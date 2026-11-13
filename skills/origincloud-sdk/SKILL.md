---
name: origincloud-sdk
description: >-
  Write code that calls Origin Cloud (origincloud.eu) with its SDKs for
  Rust (origincloud-* crates), Go (go.origincloud.eu/sdk) and TypeScript
  (@origincloud/* packages): clients per service, credentials and regions,
  waiting on long-running operations, paging through lists, retries and
  request_id, and errors. Use whenever code creates, reads, changes or
  deletes Origin Cloud instances, volumes, snapshots, VPCs, subnets,
  security groups, floating IPs, DNS zones and records, service accounts,
  role bindings, projects or billing data through an SDK.
license: Apache-2.0
metadata:
  api-revision: "227636fb9bf7bc1081c983acff81ad5b0ac70d44"
  sdk-go-version: "0.27.0"
  sdk-rust-version: "0.25.0"
  sdk-typescript-version: "0.26.0"
---
# Origin Cloud with the SDKs

The SDKs for Rust, Go and TypeScript are generated from the same API: a
client per API service, a method per call. This skill describes the
releases in `metadata` (`sdk-rust-version`, `sdk-go-version`,
`sdk-typescript-version`); pin those versions.

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

## The SDKs

**Credentials.** Code never holds a credential: make the core client from
the environment, which takes the first of `ORIGINCLOUD_ACCESS_TOKEN`;
`ORIGINCLOUD_CLIENT_ID` and `ORIGINCLOUD_CLIENT_SECRET`; a CI job's OIDC
token (`ORIGINCLOUD_OIDC_TOKEN_FILE`, `ORIGINCLOUD_OIDC_TOKEN`, or GitHub
Actions with `id-token: write`); the `origin` CLI's login; an instance's
metadata service. The region comes from `ORIGINCLOUD_REGION` or the
client's option.

| | Rust | Go | TypeScript |
|---|---|---|---|
| Core client | `origincloud_core::Client::new()?` | `core.NewClient(ctx)` | `new Client()` from `@origincloud/core` |
| Region | `Client::builder().region("eu-central1").build()?` | `core.WithRegion("eu-central1")` | `new Client({ region: "eu-central1" })` |
| Retries | `.max_attempts(n)` | `core.WithMaxAttempts(n)` | `{ maxAttempts: n }` |
| Service client | `VolumeClient::new(&client)` | `pvsv1beta1.NewVolumeClient(client)` | `new VolumeClient(client)` |

**Operations.** A method that starts a change answers with an operation:
wait for it before using its result. `op.wait().await?` (Rust),
`op.Wait(ctx)` (Go) and `await op.wait()` (TypeScript) return the result,
or the operation's error.

**Lists.** A list answers a page at a time. The `all_*` (Rust, a stream),
`All*` (Go, an iterator) and `all*` (TypeScript, an async generator)
helpers next to it read every page.

**Retries and `request_id`.** A request with an empty `request_id` gets one
generated, and the SDK retries what is safe to retry (reads, and calls with
a `request_id`) with a growing delay. To retry a change yourself, keep its
`request_id` and send it again.

**Errors.** An API error carries its code and the correlation ID: Rust
`origincloud_core::Error` (`reason()`, and the `request_id` of its
`ApiError`), Go `*core.Error` (`Reason`, `RequestID`, `core.ReasonOf(err)`),
TypeScript `OriginCloudError` (`reason`, `requestId`). Report both.

```rust
use origincloud_core::Client;
use origincloud_pvs::v1beta1::{CreateVolumeRequest, Volume, VolumeClient};

let client = Client::new()?;
let volumes = VolumeClient::new(&client);
let volume = volumes
    .create_volume(CreateVolumeRequest {
        parent: "projects/my-project/zones/eu-central1-az1".to_owned(),
        volume: Some(Volume { size_gib: 10, ..Default::default() }),
        ..Default::default()
    })
    .await?
    .wait()
    .await?;
```

```go
client, err := core.NewClient(ctx)
volumes := pvsv1beta1.NewVolumeClient(client)
op, err := volumes.CreateVolume(ctx, &pvsv1beta1.CreateVolumeRequest{
	Parent: "projects/my-project/zones/eu-central1-az1",
	Volume: &pvsv1beta1.Volume{SizeGib: 10},
})
volume, err := op.Wait(ctx)
```

```ts
import { Client } from "@origincloud/core";
import { VolumeClient } from "@origincloud/pvs/v1beta1";

const volumes = new VolumeClient(new Client());
const op = await volumes.createVolume({
  parent: "projects/my-project/zones/eu-central1-az1",
  volume: { sizeGib: 10 },
});
const volume = await op.wait();
for await (const v of volumes.allVolumes({ parent: "projects/my-project/zones/eu-central1-az1" })) {
  console.log(v.name, v.state);
}
```

## References

One file per service: its packages, each client's methods in every
language, what to ask before, and the API's resources with their full names
and fields. The service's guide on docs.origincloud.eu is linked from each
file.

<!-- BEGIN GENERATED references -->
| Service | Reference | Resources |
|---|---|---|
| billing | [references/billing.md](references/billing.md) | billingAccounts, projectBillingInfos, billingCountries, spendLimits |
| compute | [references/compute.md](references/compute.md) | instanceTypes, imageFamilies, images, instances, keyPairs, publicImages, volumeAttachments |
| dns | [references/dns.md](references/dns.md) | hostedZones, recordSets, vpcAttachments |
| iam | [references/iam.md](references/iam.md) | auditLogEntries, federatedCredentials, permissions, roles, roleBindings, serviceAccounts, users |
| location | [references/location.md](references/location.md) | regions, zones |
| network | [references/network.md](references/network.md) | acls, aclAssociations, aclRules, dnsPolicies, dnsPolicyAssociations, floatingIps, floatingIpAssociations, internetGateways, natGateways, networkInterfaces, routes, routeTables, routeTableAssociations, securityGroups, securityGroupAssociations, securityGroupRules, subnets, vpcs |
| observability | [references/observability.md](references/observability.md) | — |
| pvs | [references/pvs.md](references/pvs.md) | snapshots, storageClasses, volumes |
| resourcemanager | [references/resourcemanager.md](references/resourcemanager.md) | organizations, projects, organizationQuotas, projectQuotas |
| sts | [references/sts.md](references/sts.md) | — |
| support | [references/support.md](references/support.md) | cases, caseMessages |
<!-- END GENERATED references -->
