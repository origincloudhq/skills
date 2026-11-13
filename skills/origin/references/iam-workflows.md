# iam: workflows

Written by hand; the commands and flags are in [iam.md](iam.md). Read
every output with `--output json`. `--organization` takes the
organization's id, the last segment of its `name`.

## Give a service account a role

A role binding grants one role to one member, on the whole organization
or, with `--project`, on one project. Grant the narrowest role and scope
that does what the user asked.

1. The organization:

   ```sh
   origin resourcemanager list-organizations --output json \
     | jq -r '.organizations[] | [.name, .displayName] | @tsv'
   ```

2. The service account, by the name the user knows it by, its
   `displayName`. Its `member` field is what the binding names:

   ```sh
   origin iam list-service-accounts --organization <organization> --output json \
     | jq -r '.serviceAccounts[] | [.displayName, .member, .state, .type] | @tsv'
   ```

3. The role: the predefined role that covers what the user asked for and
   least else.

   ```sh
   origin iam list-roles --output json | jq -r '.roles[] | [.name, .displayName, .description] | @tsv'
   origin iam get-role --role <role> --output json | jq -r '.includedPermissions[]'
   ```

4. What the member holds already:

   ```sh
   origin iam list-role-bindings --organization <organization> --member <member> --output json
   ```

5. Grant it. `--project` limits the binding to that project; without it,
   the binding covers every project of the organization:

   ```sh
   origin iam create-role-binding --organization <organization> \
     --member <member> --role <role> --project projects/<project> --output json
   ```

   `CONFLICT` means the member holds the role at that scope already;
   `NOT_FOUND` that no account is that member.

Taking a role away is `origin iam delete-role-binding`, a delete: ask
first, naming the member, the role and the scope.

## Check what a caller may do

```sh
origin iam test-iam-permissions --resource projects/<project> --permission pvs.volume.create --output json
```

It answers with the permissions the caller holds out of those asked,
on the organization or a project.

## A service account for a CI job

A `WORKLOAD` service account has no secret: a CI job runs as it through a
federated credential that trusts the job's OIDC token, and logs in with
`origin id login --oidc-token-file <path>` (on GitHub Actions with
`id-token: write`, every command does it on its own).

```sh
origin iam create-service-account --organization <organization> --display-name <name> --type WORKLOAD --output json
```

Then a federated credential (`origin iam create-federated-credential`) for
the job's issuer and a condition on its claims, and a role binding as
above. The guide for GitHub Actions:
<https://docs.origincloud.eu/docs/iam/github-actions/index.html.md>.
An `EXTERNAL` service account answers with its client secret, once: the
user runs that create, not you.

## Who did what

```sh
origin iam list-audit-log-entries --organization <organization> --output json
```
