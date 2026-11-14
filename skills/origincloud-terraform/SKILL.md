---
name: origincloud-terraform
description: >-
  Write, review, import and apply Terraform or OpenTofu configurations for
  Origin Cloud (origincloud.eu) with the origincloudhq/origincloud provider.
  Use whenever HCL, `terraform` or `tofu` commands, infrastructure as code
  or state touch Origin Cloud resources: origincloud_compute_instance,
  origincloud_pvs_volume and snapshots, origincloud_network_vpc, subnets,
  security groups and rules, floating IPs, NAT and internet gateways, route
  tables, origincloud_dns_hosted_zone and record sets, IAM role bindings and
  service accounts, projects, billing accounts and spend limits.
license: Apache-2.0
metadata:
  api-revision: "227636fb9bf7bc1081c983acff81ad5b0ac70d44"
  provider-version: "0.18.0"
---
# Origin Cloud with Terraform

The provider `origincloudhq/origincloud` manages Origin Cloud's resources
from Terraform and OpenTofu. This skill describes the provider release in
`metadata.provider-version`; pin that version.

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

## Terraform

**Provider.** Credentials never go into HCL, variables, `.tfvars` or the
backend configuration: the provider reads them from the environment, in this
order: `ORIGINCLOUD_ACCESS_TOKEN`; `ORIGINCLOUD_CLIENT_ID` and
`ORIGINCLOUD_CLIENT_SECRET`; a CI job's OIDC token
(`ORIGINCLOUD_OIDC_TOKEN_FILE`, `ORIGINCLOUD_OIDC_TOKEN`, or GitHub Actions
with `id-token: write`); the `origin` CLI's login; an instance's metadata
service. The provider block takes only `region` (else `ORIGINCLOUD_REGION`)
and `endpoint`, which is for a test server the user names: never set it, or
a token, to get past a missing login; tell the user what's missing instead.

```hcl
terraform {
  required_providers {
    origincloud = {
      source = "origincloudhq/origincloud"
    }
  }
}

provider "origincloud" {
  region = "eu-central1"
}
```

**Types.** A resource is `origincloud_<service>_<resource>`, such as
`origincloud_pvs_volume`; a data source of the same name reads one by its
name, and the plural (`origincloud_pvs_volumes`) lists them. Each resource
takes the parts of its name (`project`, `zone` or `region`, `organization`)
as arguments; use variables for them rather than repeating literals. Refer
to another resource through its attributes, not by copying a name.

**What the configuration doesn't manage.** A resource the user names that
Terraform doesn't manage, such as "our VPC web", is referred to by its
`name` as the API answers it, never by one built from the name the user
gave: that is its `origin:name` tag, and its id is another, such as
`vpc-4d5e6f`. Read it first, with the `origin` CLI
(`origin network list-vpcs --output json`) or the plural data source,
filtered by the tag:

```hcl
data "origincloud_network_vpcs" "all" {
  project = var.project
  region  = var.region
}

locals {
  vpc = one([for v in data.origincloud_network_vpcs.all.vpcs : v.name
    if try(v.tags["origin:name"], "") == "web"])
}
```

**Arguments.** The references give each type's arguments: `required`,
`optional`, `computed` (set by the API, read-only) and `replaces`: changing
one destroys the resource and creates a new one. A replacement deletes
data, so it is a delete under the rules.

**Plan before apply.**
1. `terraform fmt` and `terraform validate`.
2. `terraform plan -out=tfplan`, and show the user its summary: what is
   created, changed, replaced (`-/+`, `forces replacement`) and destroyed.
3. Any destroy or replacement, and any one-way change the references list,
   needs the user's yes for it. Then `terraform apply tfplan`, that plan
   and no other. Never `-auto-approve` unless the user asked for it.

Check the plan against what the user described, and the change against the
references, not only against the plan. Changing an argument the references
mark `replaces` (a volume's `zone`, an instance's `image`) destroys the
resource and its data and creates a new one: that is a delete, and needs
the user's yes even when they asked you to apply. A resource the user calls
existing that the plan creates (`+ create`) means the state doesn't hold it:
there is no state here, or it is in a backend or workspace you don't have.
Stop and ask; applying would create a second one, and leave the first.

The provider waits for every operation itself; `apply` returns once the
resources are ready. It has no `timeouts` block.

**Import.** Bring an existing resource under Terraform with an `import`
block whose `id` is the resource's `name` (its relative name, such as
`projects/my-project/zones/eu-central1-az1/volumes/vol-1a2b3c`), never with
`terraform import`, which writes the state at once, unreviewed. Read the
resource first (`origin pvs get-volume --volume <name> --output json`, or
the service's `get-…` command) and write its arguments as it is, such as
the volume's `size_gib`: a value that differs is a change at the next
apply, and a larger `size_gib` grows the volume for good. With working
credentials, `terraform plan -generate-config-out=generated.tf` writes the
configuration instead; review it. The plan must import and change nothing
about the resource unless the user asked.

**State.** State holds every attribute, sensitive ones too: don't print it
(`terraform state show`, `terraform show -json`, `terraform output` of a
sensitive value). Don't edit it by hand. `terraform state rm`, `import` and
`-replace` change what Terraform manages: run them only when the user asks.

**Not in Terraform.** Methods that aren't a create, read, update or
delete (starting and stopping an instance, closing a support case, querying
cost) are listed per service under "Other actions". Where an argument
covers one, change the argument; otherwise run it with the `origin` CLI
when the user asks, and keep it out of the configuration.

## References

One file per service: its types, their arguments, what to ask before, and
the API's resources with their full names and fields. The service's guide
on docs.origincloud.eu is linked from each file.

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
