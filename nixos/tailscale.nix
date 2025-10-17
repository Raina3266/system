# {
#   lib,
#   config,
#   ...
# }:

# # https://tailscale.com/kb/1096/nixos-minecraft 
# {
#   options = with lib; {
#     services'.work.tailscale.enable = mkOption {
#       type = types.bool;
#       default = config.services'.work.enable;
#     };
#   };
  
#   config = lib.mkIf config.services'.work.tailscale.enable {
#     services.tailscale.enable = true;
#   };
# }
{}