# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  imports = [ ./common.nix ];

  # Use that when deploy scripts asks you for a hostname
  networking.hostName = "bare-system";
}
