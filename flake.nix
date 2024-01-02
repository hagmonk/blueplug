{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, pkgs, flake-utils, naersk, nixpkgs }:

    flake-utils.lib.eachSystem [ system.x86_64-linux ] (system: {
        nixosModule = { config, lib, pkgs, ... }:
        with lib;
        let cfg = config.services.blueplug;
        pkgs = nixpkgs.legacyPackages.${system};

        in rec {
            options.services.blueplug = {
                enable = mkEnableOption "BTLE Plug";

                client_id = mkOption {
                    type = types.str;
                    default = "";
                    description = "MQTT Client ID";
                };

                mqtt_address = mkOption {
                    type = types.str;
                    default = "";
                    description = "MQTT Address";
                };

                mqtt_port = mkOption {
                    type = types.port;
                    default = 1883;
                    description = "MQTT Port";
                };
            };

            config = mkIf cfg.enable {
                systemd.services.btleplug = {
                    description = "BTLE Plug";
                    wantedBy = ["multi-user.target"];
                    serviceConfig = {
                        ExecStart = "${pkgs.blueplug}/bin/btleplug --client_id ${cfg.client_id} --mqtt-addr ${cfg.mqtt_address} --mqtt-port ${toString cfg.mqtt_port}";
                        ProtectHome = "read-only";
                        Restart = "on-failure";
                        Type = "exec";
                    };
                };
            };

        };
    })

    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = (import nixpkgs) {
          inherit system;
        };

        naersk' = pkgs.callPackage naersk {};

      in rec {
        # For `nix build` & `nix run`:
        defaultPackage = naersk'.buildPackage {
        nativeBuildInputs = with pkgs; [ pkg-config ];
        buildInputs = with pkgs; [ openssl dbus ];
          src = ./.;
        };

        # For `nix develop`:
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [ rustc cargo ];
        };

      });
}
