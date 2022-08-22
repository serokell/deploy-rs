with import <nixpkgs> {};
with lib;
  settings: let
    mkValueString = v:
      if v == null
      then ""
      else if isInt v
      then toString v
      else if isBool v
      then boolToString v
      else if isFloat v
      then floatToString v
      else if isList v
      then toString v
      else if isDerivation v
      then toString v
      else if builtins.isPath v
      then toString v
      else if isString v
      then v
      else if isCoercibleToString v
      then toString v
      else "";

    mkKeyValue = k: v: "${escape ["="] k} = ${mkValueString v}";

    mkKeyValuePairs = attrs: concatStringsSep "\n" (mapAttrsToList mkKeyValue attrs);
  in ''
    ${mkKeyValuePairs settings}
  ''
