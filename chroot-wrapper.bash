set -e

# Re-exec ourselves in a private mount namespace so that our bind
# mounts get cleaned up automatically.
if [ -z "$NIXOS_ENTER_REEXEC" ]; then
    export NIXOS_ENTER_REEXEC=1
    if [ "$(id -u)" != 0 ]; then
        extraFlags="-r"
    fi
    exec unshare --fork --mount --uts --mount-proc --pid $extraFlags -- "$0" "$@"
else
    mount --make-rprivate /
fi

mountPoint=/mnt

while [ "$#" -gt 0 ]; do
    i="$1"; shift 1
    case "$i" in
        --root)
            mountPoint="$1"; shift 1
            ;;
        --)
            command=("$@")
            break
            ;;
        *)
            echo "$0: unknown option \`$i'"
            exit 1
            ;;
    esac
done

mkdir -p "$mountPoint/dev" "$mountPoint/sys" "$mountPoint/tmp" "$mountPoint/etc" "$mountPoint/proc"
chmod 0755 "$mountPoint/dev" "$mountPoint/sys" "$mountPoint/tmp" "$mountPoint/etc" "$mountPoint/proc"
mount --rbind /dev "$mountPoint/dev"
mount --rbind /sys "$mountPoint/sys"
mount --rbind /proc "$mountPoint/proc"

touch "$mountPoint/etc/mtab" || true # might be already a working system
(cd "$mountPoint" && ln -snf "../proc/self/mounts" "etc/mtab") # Grub needs an mtab.

export CHROOTED=1
exec chroot "$mountPoint" "${command[@]}"
