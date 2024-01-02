{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, flake-utils, naersk, nixpkgs }:

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

      })
      // {
         nixosModule = { config, lib, pkgs, ... }:
         with lib;
         let cfg = config.services.blueplug;
         in rec {
             options.services.blueplug = {
                 enable = mkEnableOption "BTLE Plug";

                systemd = mkOption {
                    type = types.bool;
                    default = pkgs.stdenv.isLinux;
                    description = "enable systemd";
                };

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
                 systemd.services.btleplug = mkIf cfg.systemd {
                     description = "BTLE Plug";
                     wantedBy = ["multi-user.target"];
                     serviceConfig = {
                         ExecStart = "${self.defaultPackage.x86_64-linux}/bin/blueplug --client-id ${cfg.client_id} --mqtt-addr ${cfg.mqtt_address} --mqtt-port ${toString cfg.mqtt_port}";
                         ProtectHome = "read-only";
                         Restart = "on-failure";
                         Type = "exec";
                     };
                 };
             };

         };
     };
}
