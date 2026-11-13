# compute: workflows

Written by hand; the commands and flags are in [compute.md](compute.md).
Read every output with `--output json`. Examples leave out `--project`,
`--zone` and `--region`: they use the profile's, so pass them when the user
works elsewhere. `<…-id>` is the last segment of a `name` alone; any other
`<…>` is a `name` or an id.

## Launch an instance with a public address

An instance reachable from the internet needs: a VPC with an internet
gateway; a subnet whose route table sends `0.0.0.0/0` to that gateway; a
network interface in the subnet, in a security group that admits the
traffic; a floating IP associated with the interface; and a key pair for
SSH. Reuse what the project has, and create only what is missing.

1. Read the network that is there:

   ```sh
   origin network list-vpcs --output json \
     | jq -r '.vpcs[] | [.name, .internetGateway, .tags["origin:name"]] | @tsv'
   origin network list-subnets --zone eu-central1-az1 --output json \
     | jq -r '.subnets[] | [.name, .vpc, .routeTable, .tags["origin:name"]] | @tsv'
   origin network list-routes --route-table <route-table-id> --output json
   ```

   A subnet reaches the internet when its `routeTable` has a route with
   `destination` `0.0.0.0/0` to its VPC's `internetGateway`.

2. Only when there is no such subnet, build one; each command waits until
   its change is recorded:

   ```sh
   origin network create-vpc --tag origin:name=<name>
   origin network create-subnet --vpc <vpc>
   origin network create-internet-gateway --vpc <vpc>
   origin network create-route-table --vpc <vpc>
   origin network create-route --route-table <route-table-id> --destination 0.0.0.0/0 --internet-gateway <internet-gateway>
   origin network create-route-table-association --subnet <subnet-id> --route-table <route-table>
   ```

3. A security group for the instance: one of the VPC's whose rules admit
   what the user asked for and nothing more, or a new one. Admit SSH from
   the range the user named, not from `0.0.0.0/0` unless they asked for it
   (see [network-workflows.md](network-workflows.md)):

   ```sh
   origin network create-security-group --vpc <vpc> --tag origin:name=<name>
   origin network create-security-group-rule --security-group <security-group-id> \
     --direction INGRESS --protocol TCP --first-port 22 --cidr-block 198.51.100.0/24
   origin network create-security-group-rule --security-group <security-group-id> \
     --direction EGRESS --protocol ALL --cidr-block 0.0.0.0/0
   ```

   Adding a rule to a group other instances use opens them too: say so, or
   use a group of the new instance's own.

4. The network interface, in the subnet and the group:

   ```sh
   nic="$(origin network create-network-interface --subnet <subnet> --output json | jq -r .name)"
   origin network create-security-group-association --network-interface "${nic##*/}" --security-group <security-group>
   ```

5. The key pair: one of the project's (`origin compute list-key-pairs`), or
   the user's own public key imported with
   `origin compute create-key-pair --public-key "$(cat ~/.ssh/id_ed25519.pub)"`.
   Without `--public-key` the platform generates a key and answers with
   its private half: that is the user's to run.

6. The instance type and image family: one whose `available` is `true`.

   ```sh
   origin compute list-instance-types --output json \
     | jq -r '.instanceTypes[] | select(.available) | [.name, .vcpu, .memoryMib] | @tsv'
   origin compute list-image-families --output json \
     | jq -r '.imageFamilies[] | select(.available) | [.name, .displayName] | @tsv'
   ```

7. Launch it. The command waits until the instance is `RUNNING`, or exits
   1 once it is `FAILED`, with the reason in `stateReason`:

   ```sh
   origin compute create-instance --instance-type gp1.core.2 --image-family debian-12 \
     --key-pair <key-pair> --network-interface "$nic" --tag origin:name=<name> --output json
   ```

8. The public address: a floating IP, associated with the interface.

   ```sh
   fip="$(origin network create-floating-ip --output json | jq -r .name)"
   origin network create-floating-ip-association --floating-ip "${fip##*/}" --network-interface "$nic"
   origin network get-floating-ip --floating-ip "$fip" --output json | jq -r .address
   ```

Tell the user the address, the instance's `loginUser` and the command,
`ssh <loginUser>@<address>`. The instance, its boot volume and the
floating IP are charged while they exist.

## What an instance uses

```sh
origin compute get-instance --instance <instance> --output json \
  | jq '{state, instanceType, networkInterfaces, interfaceAddresses}'
origin compute list-volume-attachments --instance <instance> --output json
origin network list-security-group-associations --network-interface <network-interface-id> --output json
```

## Stop, start and delete

`origin compute stop-instance` and `origin compute start-instance` wait
until the instance is `STOPPED` or `RUNNING`; it keeps its volumes and
addresses. `origin compute delete-instance` terminates it for good: ask
first, and say what goes with it (its boot volume, unless a snapshot of it
exists) and what stays and is still charged: its other volumes, detached,
and its network interfaces, released with their addresses.
