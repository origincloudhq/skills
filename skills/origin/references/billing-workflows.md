# billing: workflows

Written by hand; the commands and flags are in [billing.md](billing.md).
Read every output with `--output json`. Reading cost and usage needs
`admin.usage.list` on the organization; a `PERMISSION_DENIED` is reported,
not worked around.

## Read a project's cost

The cost of a period, by service and by product, in daily (or
`--granularity HOURLY`) buckets. Amounts are EUR before VAT, as decimal
strings: report them as the API gave them, and add them up exactly, not
in floating point. The period is `--start-time` (included) to
`--end-time` (left out), in RFC 3339 and UTC; this month so far:

```sh
start="$(date -u +%Y-%m-01T00:00:00Z)"
now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
origin billing query-project-cost --project <project> --start-time "$start" --end-time "$now" --output json \
  | jq -r '"total \(.total)", (.services[] | "\(.service) \(.total)"),
           ([.services[].products[]] | max_by(.total | tonumber) | "largest \(.productKey) \(.total)")'
```

`origin billing query-organization-cost --organization <organization>`
answers with every project's together and each project's on its own, and
`origin billing query-billing-account-cost --billing-account <billing-account>`
with what one billing account is charged.

## Find what a cost comes from

The usage records a cost is priced from, each a resource's metered
interval:

```sh
origin billing query-project-usage --project <project> --start-time "$start" --end-time "$now" --output json \
  | jq -r '.usageRecords[] | [.productKey, .resourceKind, .resourceId, .metric, .quantity] | @tsv'
```

`resourceId` is the resource's id; find it with the service's list, such
as `origin compute list-instances`. The prices themselves:
`origin billing get-price-list --output json`.

## Spend limits

```sh
origin billing list-billing-accounts --organization <organization> --output json
origin billing get-spend-limit --billing-account <billing-account> --organization <organization> --output json
```

Changing a spend limit (`origin billing update-spend-limit`) changes what
the organization may spend: do it only when the user asked, with the
values they named.
