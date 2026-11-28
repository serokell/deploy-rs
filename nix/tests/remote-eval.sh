# SPDX-FileCopyrightText: 2026 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

set -euo pipefail

export REAL_NIX
REAL_NIX=$(command -v nix)
export PIN=github:example/deploy/0123456789abcdef0123456789abcdef01234567
export STORE=/nix/store/00000000000000000000000000000000
export EVENTS="$PWD/events"
mkdir -p mock-bin

{
    printf '#!%s\n' "$BASH"
    cat <<'MOCK'
set -euo pipefail

case ${1:-} in
    eval)
        if [[ $* == *builtins.getFlake* ]]; then
            [[ $4 == --expr && $5 == "(builtins.getFlake \"$PIN\").deploy" ]]
            shift 5
            exec "$REAL_NIX" eval --store dummy:// --json --expr '
              {
                remoteEval = true;
                nodes.node = {
                  hostname = "target";
                  sshUser = "builder";
                  user = "owner";
                  sshOpts = [ "-p" "2222" ];
                  profilesOrder = [ "local" "remote" ];
                  profiles = {
                    local = {
                      remoteEval = false;
                      path = "/nix/store/00000000000000000000000000000000-local";
                      drvPath = "/nix/store/00000000000000000000000000000000-local.drv";
                    };
                    remote.path = throw "controller forced remote path";
                  };
                };
              }
            ' "$@"
        fi
        exec "$REAL_NIX" --store dummy:// "$@"
        ;;
    path-info) printf '%s\n' "$2" ;;
    build) printf '%s\n' "$STORE-local" ;;
    copy) echo pushed-local >>"$EVENTS" ;;
    --extra-experimental-features)
        [[ ${REMOTE_SHELL:-} == yes && $2 == 'nix-command flakes' ]]
        shift 2
        [[ $# == 6 && $1 == build && $2 == --no-link && $3 == --print-out-paths ]]
        [[ $4 == --no-update-lock-file && $5 == --expr ]]
        [[ $6 == "(builtins.getFlake \"$PIN\").deploy.nodes.\"node\".profiles.\"remote\".path.out" ]]
        echo built-remote >>"$EVENTS"
        if [[ ${FAIL_BUILD:-} == yes ]]; then
            echo "intentional remote build failure" >&2
            exit 1
        fi
        printf '%s\n' "$STORE-remote"
        ;;
    *) echo "Unexpected nix invocation: $*" >&2; exit 98 ;;
esac
MOCK
} >mock-bin/nix

{
    printf '#!%s\n' "$BASH"
    cat <<'MOCK'
set -euo pipefail

if [[ $1 == -T ]]; then
    [[ $# == 6 && $2 == -p && $3 == 2222 && $4 == -- && $5 == builder@target ]]
    REMOTE_SHELL=yes bash -c "$6"
else
    [[ $# == 4 && $1 == builder@target && $2 == -p && $3 == 2222 ]]
    [[ $4 == "sudo -u owner "* && $4 == *" --dry-activate"* ]]
    grep -qx pushed-local "$EVENTS"
    grep -qx built-remote "$EVENTS"
    case $4 in
        *"$STORE-local/activate-rs"*) echo activate-local >>"$EVENTS" ;;
        *"$STORE-remote/activate-rs"*) echo activate-remote >>"$EVENTS" ;;
        *) exit 97 ;;
    esac
fi
MOCK
} >mock-bin/ssh
chmod +x mock-bin/nix mock-bin/ssh
export PATH="$PWD/mock-bin:$PATH"

if FAIL_BUILD=yes deploy --dry-activate --skip-checks --no-progress "$PIN#node" >fail.log 2>&1; then
    echo "Expected the remote build to fail" >&2
    exit 1
fi
grep 'intentional remote build failure' fail.log
grep 'Failed to build profile remote on node node' fail.log
if grep -q '^activate-' "$EVENTS"; then
    echo "Activation ran before a successful build" >&2
    exit 1
fi

: >"$EVENTS"
deploy --dry-activate --skip-checks --no-progress "$PIN#node"
diff -u <(printf 'activate-local\nactivate-remote\n') <(grep '^activate-' "$EVENTS")

: >"$EVENTS"
if deploy --dry-activate --no-progress "$PIN#node" >checks.log 2>&1; then
    echo "Expected remote evaluation without --skip-checks to fail" >&2
    exit 1
fi
grep 'remoteEval requires --skip-checks' checks.log
test ! -s "$EVENTS"

if deploy --dry-activate --skip-checks --remote-eval=false "$PIN#node.remote" >override.log 2>&1; then
    echo "Expected the CLI override to evaluate the remote profile on the controller" >&2
    exit 1
fi
grep 'controller forced remote path' override.log
test ! -s "$EVENTS"
