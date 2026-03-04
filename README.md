# Shadow Dargon

Shargon(Shadow Dargon) is a tool designed to instantly spawn lightweight, completely isolated virtual machines for end-to-end (E2E) testing.

WIP

Two backend are planned:
- `systemd-nspawn` (Best with `btrfs` or `xfs` file system)
- `qemu` (use snapshot to bypass the boot sequence entirely)

Needs more investigation:
- `docker` (not recommended though. `checkpoint` uses `criu`(which is fragile) and it's very experimental; disk snapshots using `commit` command still requires cold start)
- `firecracker`
