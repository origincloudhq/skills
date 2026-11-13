# network: workflows

Written by hand; the commands and flags are in [network.md](network.md).
Read every output with `--output json`. Examples leave out `--project`,
`--zone` and `--region`: they use the profile's, so pass them when the user
works elsewhere. `<…-id>` is the last segment of a `name` alone; any other
`<…>` is a `name` or an id.

## Allow a port or ICMP in a security group

Security groups are stateful: a rule that admits a connection admits its
replies too, and what no rule allows is denied. A rule can't be changed;
add one, and remove one only with the user's yes.

1. Find the group. By the name the user knows it by, its `origin:name`
   tag, or through the instance whose traffic it is:

   ```sh
   origin network list-security-groups --output json \
     | jq -r '.securityGroups[] | [.name, .vpc, .tags["origin:name"]] | @tsv'
   origin compute get-instance --instance <instance> --output json | jq -r '.networkInterfaces[]'
   origin network list-security-group-associations --network-interface <network-interface-id> --output json \
     | jq -r '.securityGroupAssociations[].securityGroup'
   ```

2. Read its rules; a rule that already admits the traffic needs nothing
   more:

   ```sh
   origin network list-security-group-rules --security-group <security-group-id> --output json \
     | jq -r '.securityGroupRules[] | [.direction, .protocol, .firstPort, .lastPort, (.cidrBlock // .peerSecurityGroup // "same group"), .description] | @tsv'
   ```

3. Add the rule. The peer is the narrowest the user asked for: a range
   with `--cidr-block`, another group of the same VPC with
   `--peer-security-group`, or the group's own members with
   `--same-group true`. `0.0.0.0/0` is anyone over IPv4, `::/0` over IPv6:
   a rule for each when the instances have both.

   ```sh
   # One port: --first-port alone.
   origin network create-security-group-rule --security-group <security-group-id> \
     --direction INGRESS --protocol TCP --first-port 443 --cidr-block 0.0.0.0/0 --description "HTTPS"
   # A range: --first-port below --last-port.
   origin network create-security-group-rule --security-group <security-group-id> \
     --direction INGRESS --protocol UDP --first-port 60000 --last-port 61000 --cidr-block 203.0.113.0/24
   # Ping and other ICMP (ICMPv6 for an IPv6 peer); ICMP takes no ports.
   origin network create-security-group-rule --security-group <security-group-id> \
     --direction INGRESS --protocol ICMP --cidr-block 0.0.0.0/0 --description "ping"
   ```

   Each spelling of a set of ports is the one: `--first-port 22
   --last-port 22` is refused with `INVALID_ARGUMENT`, which names the one
   to use. A rule the group already has, `--description` included, is
   refused with `FAILED_PRECONDITION`.

4. Every instance whose interface is in the group is opened, not only
   the one the user named: tell them which, from
   `origin network list-network-interfaces --output json`
   (`securityGroups`, `instance`).

## Narrow a rule

There is no change in place: add the narrower rule, then delete the
broader one with `origin network delete-security-group-rule --security-group <security-group-id> --rule <rule> --yes`
once the user agreed to that rule's removal.

## Remove a security group

`origin network delete-security-group` deletes the group with its rules.
It is refused while an association names it, and for a VPC's default
group. Before asking the user, find what is in it:

```sh
origin network list-network-interfaces --output json \
  | jq -r --arg sg <security-group-name> '.networkInterfaces[] | select(.securityGroups | index($sg)) | [.name, .instance] | @tsv'
```

Tell the user the group, its rules and the interfaces in it, and wait for
their yes.

## Public addresses

```sh
origin network list-floating-ips --output json \
  | jq -r '.floatingIps[] | [.name, .address, .networkInterface] | @tsv'
```

A floating IP with no `networkInterface` is associated with nothing and
still charged.
