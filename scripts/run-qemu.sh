#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ISO_PATH="$ROOT_DIR/dist/nexaos.iso"
ROOTFS_IMG="$ROOT_DIR/build/rootfs.ext2"
SMP_CORES="${SMP:-4}"

# Parse script arguments and treat them as additional QEMU arguments.
# The user can either pass QEMU args directly, or separate script args and
# QEMU args using `--`. Example: `./scripts/run-qemu.sh -S -s` or
# `./scripts/run-qemu.sh -- -S -s`.
EXTRA_QEMU_ARGS=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            cat <<'USAGE'
Usage: run-qemu.sh [--] [<qemu-args>...]

Run NexaOS in QEMU with bridge networking (VM on same network as host).

Environment variables:
  SMP=<N>          Number of CPU cores (default: 4)
  BIOS_MODE=uefi   Use UEFI boot (default)
  BIOS_MODE=legacy Use legacy BIOS boot

Examples:
  # Start QEMU with bridge networking
  ./scripts/run-qemu.sh

  # Run QEMU with GDB server and paused CPU (-S -s)
  ./scripts/run-qemu.sh -S -s
USAGE
            exit 0
            ;;
        --)
            shift
            EXTRA_QEMU_ARGS+=("$@")
            break
            ;;
        *)
            EXTRA_QEMU_ARGS+=("$1")
            shift
            ;;
    esac
done

# Check for ISO
if [[ ! -f "$ISO_PATH" ]]; then
    echo "ISO image not found at $ISO_PATH. Run scripts/build-iso.sh first." >&2
    exit 1
fi

# Root filesystem now required (initramfs no longer embeds it)
if [[ ! -f "$ROOTFS_IMG" ]]; then
    echo "Root filesystem image missing at $ROOTFS_IMG." >&2
    echo "Run scripts/build-rootfs.sh (or build-all.sh) before launching QEMU." >&2
    exit 1
fi

echo "Starting NexaOS in QEMU..."
echo "  Kernel: via ISO"
echo "  Root device: ${ROOTFS_IMG}"

# 默认使用UEFI启动
DEFAULT_BIOS_MODE="${BIOS_MODE:-uefi}"

# Don't build QEMU_CMD yet - need to setup network first to get NET_DEVICE
echo "  Boot mode: ${DEFAULT_BIOS_MODE^^}"
echo "  Virtio block device attached as /dev/vda"
echo "  Network: Bridge mode (VM on same network as host)"
echo "  Kernel parameters should include: root=/dev/vda1 rootfstype=ext2"

if [[ "$DEFAULT_BIOS_MODE" == "uefi" ]]; then
    # UEFI 启动模式（默认）
    CAND_DIRS=(/usr/share/OVMF /usr/share/ovmf /usr/share/edk2/ovmf)

    UEFI_CODE=""
    UEFI_VARS_TEMPLATE=""

    # Search for code firmware (matches OVMF_CODE*.fd, including OVMF_CODE_4M.fd etc.)
    for d in "${CAND_DIRS[@]}"; do
        for f in "$d"/OVMF_CODE*.fd; do
            if [[ -f "$f" ]]; then
                UEFI_CODE="$f"
                break 2
            fi
        done
    done

    # Search for vars template (matches OVMF_VARS*.fd, including OVMF_VARS_4M.fd etc.)
    for d in "${CAND_DIRS[@]}"; do
        for f in "$d"/OVMF_VARS*.fd; do
            if [[ -f "$f" ]]; then
                UEFI_VARS_TEMPLATE="$f"
                break 2
            fi
        done
    done

    if [[ -z "$UEFI_CODE" || -z "$UEFI_VARS_TEMPLATE" ]]; then
        echo "OVMF firmware not found. Install edk2-ovmf (package name may vary) and retry." >&2
        exit 1
    fi

    UEFI_VARS_COPY="$ROOT_DIR/build/OVMF_VARS.fd"
    mkdir -p "$ROOT_DIR/build"
    if [[ ! -f "$UEFI_VARS_COPY" ]]; then
        cp "$UEFI_VARS_TEMPLATE" "$UEFI_VARS_COPY"
    fi
fi

echo "  SMP cores: ${SMP_CORES}"

# Network setup - Use macvtap for WiFi interfaces, bridge for Ethernet
echo ""
echo "========================================================================"
echo "Setting up network for VM..."
echo "========================================================================"

# Get default network interface
DEFAULT_IF=$(ip route | grep default | awk '{print $5}' | head -n1)

if [[ -z "$DEFAULT_IF" ]]; then
    echo "ERROR: Could not find default network interface" >&2
    exit 1
fi

echo "Default interface: $DEFAULT_IF"

# Check if it's a wireless interface
IS_WIRELESS=false
if [[ -d "/sys/class/net/$DEFAULT_IF/wireless" ]] || iwconfig "$DEFAULT_IF" 2>/dev/null | grep -q "ESSID"; then
    IS_WIRELESS=true
    echo "Detected wireless interface - WiFi L2 bridging not supported by 802.11 protocol"
    echo "Using QEMU user-mode networking (default 10.0.2.0/24 subnet)"
    
    # User-mode networking: VM gets DHCP (typically 10.0.2.15), gateway 10.0.2.2, DNS 10.0.2.3
    NET_MODE="user"
    VM_MAC="52:54:00:12:34:56"
else
    echo "Detected wired interface - using TAP bridge mode"
    
    # Remove existing tap0 if exists
    sudo ip link delete tap0 2>/dev/null || true
    
    # Use traditional TAP + bridge for Ethernet
    echo "Creating TAP device..."
    sudo ip tuntap add dev tap0 mode tap user "$(whoami)"
    sudo ip link set tap0 up promisc on
    
    # Check if bridge already exists
    if ip link show br0 &>/dev/null; then
        echo "Using existing bridge br0"
    else
        echo "Creating bridge br0..."
        sudo ip link add name br0 type bridge
        sudo ip link set br0 up
        
        # Move IP from physical interface to bridge
        IP_ADDR=$(ip addr show "$DEFAULT_IF" | grep "inet " | awk '{print $2}')
        if [[ -n "$IP_ADDR" ]]; then
            echo "Moving IP $IP_ADDR from $DEFAULT_IF to br0..."
            sudo ip addr del "$IP_ADDR" dev "$DEFAULT_IF" 2>/dev/null || true
            sudo ip addr add "$IP_ADDR" dev br0
        fi
        
        # Add physical interface to bridge
        sudo ip link set "$DEFAULT_IF" master br0
        
        # Update default route
        GW=$(ip route | grep default | awk '{print $3}' | head -n1)
        if [[ -n "$GW" ]]; then
            sudo ip route del default 2>/dev/null || true
            sudo ip route add default via "$GW" dev br0
        fi
    fi
    
    # Add TAP to bridge
    sudo ip link set tap0 master br0
    echo "tap0 added to bridge br0"
    echo "VM will be on the same network as $DEFAULT_IF (Ethernet)"
    
    NET_MODE="tap"
    NET_DEVICE="tap0"
    VM_MAC="52:54:00:12:34:56"
fi

# Export for QEMU to use
export NET_MODE
export NET_DEVICE
export VM_MAC
export VM_IP
export HOST_IP
export DNS_IP

echo ""
echo "Network setup complete!"
if [[ "$NET_MODE" == "user" ]]; then
    echo "  Mode: user-mode networking (WiFi workaround)"
    echo "  VM will receive IP via DHCP (typically 10.0.2.15)"
    echo "  Gateway: 10.0.2.2 (via DHCP option 3)"
    echo "  DNS: 10.0.2.3 (via DHCP option 6, forwards to host resolver)"
    echo "  Note: VM can access internet via NAT, host can access VM via port forwarding"
else
    echo "  Mode: bridge (Ethernet)"
    echo "  Device: $NET_DEVICE"
    echo "  VM MAC: ${VM_MAC}"
    echo "  VM will receive IP from your network's DHCP server"
    echo "  VM will be on the same L2 network as your host"
    echo ""
    echo "To cleanup: sudo ip link delete $NET_DEVICE"
fi
echo "========================================================================"
echo ""

# Now build QEMU command with the correct network device
if [[ "$DEFAULT_BIOS_MODE" == "legacy" ]]; then
    QEMU_CMD=(
        qemu-system-x86_64
        -m 1G
        -serial stdio
        -smp "$SMP_CORES"
        -vga std
        -display gtk,window-close=on
        -cdrom "$ISO_PATH"
        -d guest_errors
        -monitor none
        -drive file="$ROOTFS_IMG",id=rootfs,format=raw,if=none
        -device virtio-blk-pci,drive=rootfs
    )
    
    # Add network configuration based on mode
    if [[ "$NET_MODE" == "user" ]]; then
        QEMU_CMD+=(
            -netdev "user,id=net0"
            -device "e1000,netdev=net0,mac=${VM_MAC}"
        )
    else
        QEMU_CMD+=(
            -netdev "tap,id=net0,ifname=${NET_DEVICE},script=no,downscript=no"
            -device "e1000,netdev=net0,mac=${VM_MAC}"
        )
    fi
else
    QEMU_CMD=(
        qemu-system-x86_64
        -m 1G
        -serial stdio
        -smp "$SMP_CORES"
        -vga std
        -display gtk,window-close=on
        # UEFI firmware: code (readonly) and writable vars copy
        -drive if=pflash,format=raw,readonly=on,file="$UEFI_CODE"
        -drive if=pflash,format=raw,file="$UEFI_VARS_COPY"
        -cdrom "$ISO_PATH"
        -d guest_errors
        -monitor none
        -drive file="$ROOTFS_IMG",id=rootfs,format=raw,if=none
        -device virtio-blk-pci,drive=rootfs
    )
    
    # Add network configuration based on mode
    if [[ "$NET_MODE" == "user" ]]; then
        QEMU_CMD+=(
            -netdev "user,id=net0"
            -device "e1000,netdev=net0,mac=${VM_MAC}"
        )
    else
        QEMU_CMD+=(
            -netdev "tap,id=net0,ifname=${NET_DEVICE},script=no,downscript=no"
            -device "e1000,netdev=net0,mac=${VM_MAC}"
        )
    fi
fi

# If additional QEMU args were supplied, forward them to QEMU.
if [[ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]]; then
    echo "Additional QEMU args: ${EXTRA_QEMU_ARGS[*]}"
    QEMU_CMD+=("${EXTRA_QEMU_ARGS[@]}")
fi

# Run QEMU
exec "${QEMU_CMD[@]}"