# pvs: workflows

Written by hand; the commands and flags are in [pvs.md](pvs.md). Read every
output with `--output json`. Examples leave out `--project` and `--zone`:
they use the profile's, so pass them when the user works elsewhere.
`<…>` is a `name` or an id.

## Grow a volume and see what holds it

Growing a volume can't be undone: it never shrinks back, and the larger
size is charged. It grows the disk, not the file system on it.

1. Read the volume:

   ```sh
   origin pvs get-volume --volume <volume> --output json \
     | jq '{name, sizeGib, state, attachment, resizeState, pendingSizeGib}'
   ```

   A `resizeState` of `RESIZING` means a resize is under way, to
   `pendingSizeGib`: wait for it rather than starting another.

2. When it is `IN_USE`, find the instance that holds it:

   ```sh
   origin compute list-volume-attachments --instance - --zone <zone> --volume <volume> --output json \
     | jq -r '.volumeAttachments[] | [.name, .device] | @tsv'
   ```

   The attachment's name holds the instance's,
   `projects/…/zones/…/instances/<instance>/volumeAttachments/…`, and
   `device` is the disk the guest sees, such as `vdb`.

3. Ask. Tell the user the volume, its size now and after, that it can't
   shrink back and the larger size is charged, and which instance holds it.
   Wait for their yes.

4. Grow it. The command waits until the volume reports the new size:

   ```sh
   origin pvs update-volume --volume <volume> --size-gib 250 --yes --output json
   ```

5. Tell the user that the file system on the device is grown inside the
   instance, such as with `growpart` and `resize2fs` or `xfs_growfs`, and
   that you haven't done that unless they asked you to and you can reach
   the instance.

## Snapshot a volume before a risky change

1. Take the snapshot, tagged so that it is found again. The command waits
   until it is `COMPLETED`:

   ```sh
   origin pvs create-snapshot --source-volume <volume> --tag origin:name=<volume>-before-upgrade --output json
   ```

   It copies the blocks as they are at that moment, like a power cut: a
   database or file system that holds writes in memory is consistent after
   a recovery, not as of a clean shutdown. For that, the user stops the
   writer first.

2. Report the snapshot's `name`.

3. When the snapshot is refused, report the error's `reason` and
   `requestId`. Don't delete an older snapshot to make room unless the
   user names it.

## Restore from a snapshot

A snapshot isn't written back over a volume: a new volume is created from
it, and the instance uses that one instead.

```sh
origin pvs protect-snapshot --snapshot <snapshot>
origin pvs create-volume --source-snapshot <snapshot> --size-gib <size> --tag origin:name=<name> --output json
```

A snapshot must be protected before a volume is created from it, and a
protected one can't be deleted until `origin pvs unprotect-snapshot`. The
new volume is at least the snapshot's `sourceSizeGib`. Swapping it in
detaches the old volume from the instance (`origin compute delete-volume-attachment`,
which needs a yes) and attaches the new one
(`origin compute create-volume-attachment`).

## Volumes that hold nothing

```sh
origin pvs list-volumes --output json \
  | jq -r '.volumes[] | select(.state == "AVAILABLE") | [.name, .sizeGib, .tags["origin:name"]] | @tsv'
```

`AVAILABLE` volumes are attached to nothing and still charged. Deleting
one needs a yes; a volume with snapshots left can't be deleted.
