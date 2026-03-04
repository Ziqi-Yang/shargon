```sh
nixos-rebuild build-image --image-variant qemu --flake .#test-vm
```

```sh
qemu-system-x86_64 -m 2048 -smp 2 -enable-kvm -drive file=./nixos.qcow2,if=virtio,format=qcow2 -device virtio-net-pci,netdev=net0 -netdev user,id=net0,hostfwd=tcp::2222-:22 -qmp unix:/tmp/qmp.sock,server,wait=off -monitor stdio
qemu-system-x86_64 -m 2048 -smp 2 -enable-kvm -drive file=./nixos.qcow2,if=virtio,format=qcow2 -device virtio-net-pci,netdev=net0 -netdev user,id=net0,hostfwd=tcp::2222-:22 -qmp unix:/tmp/qmp.sock,server,wait=off -monitor stdio  -loadvm baseline
```

```nix
{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
    in
    {
      nixosConfigurations.test-vm = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          (
            {
              pkgs,
              lib,
              modulesPath,
              ...
            }:
            {
              imports = [ "${modulesPath}/profiles/qemu-guest.nix" ];

              # virtualisation = {
              #   # useBootLoader = false;
              #   # useNixStoreImage = true;
              #   # writableStore = true;
              #   sharedDirectories = lib.mkForce { };
              #   qemu = {
              #     options = [
              #       "-qmp unix:/tmp/qmp.sock,server,wait=off"
              #       "-monitor stdio"
              #     ];
              #   };
              # };

              users.users.root.initialPassword = "pass@123!";

              networking = {
                hostName = "testvm";
                firewall.enable = false;
              };

              services = {
                postgresql = {
                  enable = true;
                  ensureDatabases = [ "web_meet" ];
                  enableTCPIP = true;
                  authentication = pkgs.lib.mkOverride 10 ''
                    local all      all     trust
                    host  all      all     127.0.0.1/32   trust
                    host  all      all     ::1/128        trust
                  '';
                };

                rabbitmq =
                  let
                    rabbitmqDefinitions = pkgs.writeText "rabbitmq-definitions.json" (
                      builtins.toJSON {
                        users = [
                          {
                            name = "root";
                            password = "pass@123!";
                            tags = "administrator";
                          }
                        ];
                        vhosts = [
                          { name = "/"; }
                        ];
                        permissions = [
                          {
                            user = "root";
                            vhost = "/";
                            configure = ".*";
                            write = ".*";
                            read = ".*";
                          }
                        ];
                      }
                    );
                  in
                  {
                    enable = true;
                    listenAddress = "127.0.0.1";
                    port = 29656;
                    configItems = {
                      "load_definitions" = "${rabbitmqDefinitions}";
                    };
                  };

                livekit = {
                  enable = true;
                  keyFile = pkgs.writeText "livekitKeyFile.txt" ''
                    APIzErDaPoqqA2Y: Ow1VbZLRsC1eDpdFfswveg6OQUt9pa7ybCSo4wWrMacB
                  '';
                };
              };

              system.stateVersion = "25.11";
            }
          )
        ];
      };
    };
}
```
