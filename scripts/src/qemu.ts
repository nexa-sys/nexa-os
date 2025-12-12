/**
 * NexaOS Build System - QEMU Configuration
 * Handles QEMU config loading and run-qemu.sh generation
 */

import { readFile, writeFile, chmod, mkdir } from 'fs/promises';
import { existsSync, readFileSync } from 'fs';
import { parse as parseYaml } from 'yaml';
import { join } from 'path';
import { BuildEnvironment } from './types.js';
import { logger } from './logger.js';

// =============================================================================
// QEMU Configuration Types
// =============================================================================

export interface QemuMachineConfig {
  arch: string;
  memory: string;
  smp: number;
  type?: string;
}

export interface QemuBootConfig {
  mode: 'uefi' | 'legacy';
  uefi_paths: string[];
}

export interface QemuBootConfigParams {
  root: string;
  rootfstype: string;
  init: string;
  console: string | null;
  quiet: boolean;
  debug: boolean;
}

export interface QemuDisplayConfig {
  vga: string;
  backend: string;
  window_close: boolean;
}

export interface QemuSerialConfig {
  output: string;
  monitor: boolean;
}

export interface QemuStorageDeviceConfig {
  path: string;
  format: string;
  cache: string;
  device: string;
}

export interface QemuStorageConfig {
  iso: string;
  rootfs: QemuStorageDeviceConfig;
  swap: QemuStorageDeviceConfig;
}

export interface QemuUserNetConfig {
  hostfwd: string[];
}

export interface QemuNetworkConfig {
  enabled: boolean;
  model: string;
  mac: string;
  mode: 'auto' | 'user' | 'bridge' | 'tap';
  bridge: string;
  tap: string;
  user: QemuUserNetConfig;
}

export interface QemuGdbConfig {
  enabled: boolean;
  port: number;
  pause: boolean;
}

export interface QemuDebugConfig {
  guest_errors: boolean;
  gdb: QemuGdbConfig;
  trace: string | null;
}

export interface QemuAudioConfig {
  enabled: boolean;
  driver: string;
  model: string;
}

export interface QemuUsbConfig {
  enabled: boolean;
  controller: string;
  devices: string[];
}

export interface QemuProfileOverride {
  description: string;
  machine?: Partial<QemuMachineConfig>;
  boot?: Partial<QemuBootConfig>;
  display?: Partial<QemuDisplayConfig>;
  serial?: Partial<QemuSerialConfig>;
  network?: Partial<QemuNetworkConfig>;
  debug?: Partial<QemuDebugConfig>;
  audio?: Partial<QemuAudioConfig>;
  usb?: Partial<QemuUsbConfig>;
}

export interface QemuConfig {
  machine: QemuMachineConfig;
  boot: QemuBootConfig;
  boot_config: QemuBootConfigParams;
  display: QemuDisplayConfig;
  serial: QemuSerialConfig;
  storage: QemuStorageConfig;
  network: QemuNetworkConfig;
  debug: QemuDebugConfig;
  audio: QemuAudioConfig;
  usb: QemuUsbConfig;
  profiles: Record<string, QemuProfileOverride>;
}

// =============================================================================
// Configuration Loading
// =============================================================================

let cachedQemuConfig: QemuConfig | null = null;

/**
 * Load QEMU configuration from config/qemu.yaml (synchronous version)
 * Use this when async is not available (e.g., in createBuildEnvironment)
 */
export function loadQemuConfigSync(projectRoot: string): QemuConfig {
  if (cachedQemuConfig) {
    return cachedQemuConfig;
  }

  const configPath = join(projectRoot, 'config', 'qemu.yaml');
  
  if (!existsSync(configPath)) {
    throw new Error(`QEMU configuration not found at ${configPath}`);
  }

  const content = readFileSync(configPath, 'utf-8');
  cachedQemuConfig = parseYaml(content) as QemuConfig;
  
  return cachedQemuConfig;
}

/**
 * Get root filesystem image path from qemu.yaml configuration
 */
export function getRootfsImgPath(projectRoot: string): string {
  const config = loadQemuConfigSync(projectRoot);
  return join(projectRoot, config.storage.rootfs.path);
}

/**
 * Get root filesystem type from qemu.yaml configuration
 */
export function getRootfsType(projectRoot: string): string {
  const config = loadQemuConfigSync(projectRoot);
  return config.boot_config.rootfstype;
}

/**
 * Load QEMU configuration from config/qemu.yaml
 */
export async function loadQemuConfig(projectRoot: string): Promise<QemuConfig> {
  if (cachedQemuConfig) {
    return cachedQemuConfig;
  }

  const configPath = join(projectRoot, 'config', 'qemu.yaml');
  
  if (!existsSync(configPath)) {
    throw new Error(`QEMU configuration not found at ${configPath}`);
  }

  const content = await readFile(configPath, 'utf-8');
  cachedQemuConfig = parseYaml(content) as QemuConfig;
  
  return cachedQemuConfig;
}

/**
 * Apply a profile to the base configuration
 */
export function applyQemuProfile(config: QemuConfig, profileName: string): QemuConfig {
  if (profileName === 'default' || !config.profiles[profileName]) {
    return config;
  }

  const profile = config.profiles[profileName];
  const result = { ...config };

  // Deep merge profile overrides
  if (profile.machine) {
    result.machine = { ...result.machine, ...profile.machine };
  }
  if (profile.boot) {
    result.boot = { ...result.boot, ...profile.boot };
  }
  if (profile.display) {
    result.display = { ...result.display, ...profile.display };
  }
  if (profile.serial) {
    result.serial = { ...result.serial, ...profile.serial };
  }
  if (profile.network) {
    result.network = { ...result.network, ...profile.network };
  }
  if (profile.debug) {
    result.debug = { 
      ...result.debug, 
      ...profile.debug,
      gdb: { ...result.debug.gdb, ...profile.debug.gdb }
    };
  }
  if (profile.audio) {
    result.audio = { ...result.audio, ...profile.audio };
  }
  if (profile.usb) {
    result.usb = { ...result.usb, ...profile.usb };
  }

  return result;
}

// =============================================================================
// Script Generation
// =============================================================================

/**
 * Generate the run-qemu.sh script from configuration
 */
export async function generateQemuScript(env: BuildEnvironment, profile: string = 'default'): Promise<void> {
  const config = await loadQemuConfig(env.projectRoot);
  const finalConfig = applyQemuProfile(config, profile);
  
  const script = generateQemuBashScript(finalConfig, env);
  
  // Ensure build directory exists
  await mkdir(env.buildDir, { recursive: true });
  
  const scriptPath = join(env.buildDir, 'run-qemu.sh');
  await writeFile(scriptPath, script, 'utf-8');
  await chmod(scriptPath, 0o755);
  
  logger.success(`Generated ${scriptPath}`);
}

/**
 * Generate the bash script content
 */
function generateQemuBashScript(config: QemuConfig, _env: BuildEnvironment): string {
  const lines: string[] = [];
  
  // Header
  lines.push('#!/usr/bin/env bash');
  lines.push('# Auto-generated by NexaOS Build System (ndk)');
  lines.push('# Configuration: config/qemu.yaml');
  lines.push('# Do not edit directly - modify config/qemu.yaml instead');
  lines.push('');
  lines.push('set -euo pipefail');
  lines.push('');
  
  // Variables
  lines.push('ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"');
  lines.push(`ISO_PATH="$ROOT_DIR/${config.storage.iso}"`);
  lines.push(`ROOTFS_IMG="$ROOT_DIR/${config.storage.rootfs.path}"`);
  lines.push(`SWAP_IMG="$ROOT_DIR/${config.storage.swap.path}"`);
  lines.push(`SMP_CORES="\${SMP:-${config.machine.smp}}"`);
  lines.push(`MEMORY="\${MEMORY:-${config.machine.memory}}"`);
  lines.push(`BIOS_MODE="\${BIOS_MODE:-${config.boot.mode}}"`);
  lines.push('');
  
  // Help function
  lines.push('show_help() {');
  lines.push('  cat <<\'USAGE\'');
  lines.push('Usage: run-qemu.sh [OPTIONS] [--] [<qemu-args>...]');
  lines.push('');
  lines.push('Run NexaOS in QEMU.');
  lines.push('');
  lines.push('Options:');
  lines.push('  -h, --help       Show this help message');
  lines.push('  -d, --debug      Enable GDB server and pause at start');
  lines.push('  -n, --no-net     Disable networking');
  lines.push('  --headless       Run without display');
  lines.push('');
  lines.push('Environment variables:');
  lines.push(`  SMP=<N>          Number of CPU cores (default: ${config.machine.smp})`);
  lines.push(`  MEMORY=<SIZE>    Memory size (default: ${config.machine.memory})`);
  lines.push('  BIOS_MODE=uefi   Use UEFI boot (default)');
  lines.push('  BIOS_MODE=legacy Use legacy BIOS boot');
  lines.push('');
  lines.push('Examples:');
  lines.push('  # Start QEMU normally');
  lines.push('  ./build/run-qemu.sh');
  lines.push('');
  lines.push('  # Start with GDB server');
  lines.push('  ./build/run-qemu.sh --debug');
  lines.push('');
  lines.push('  # Pass extra QEMU args');
  lines.push('  ./build/run-qemu.sh -- -S -s');
  lines.push('USAGE');
  lines.push('}');
  lines.push('');
  
  // Argument parsing
  lines.push('# Parse arguments');
  lines.push('DEBUG_MODE=false');
  lines.push('NO_NET=false');
  lines.push('HEADLESS=false');
  lines.push('EXTRA_QEMU_ARGS=()');
  lines.push('');
  lines.push('while [[ $# -gt 0 ]]; do');
  lines.push('  case "$1" in');
  lines.push('    -h|--help)');
  lines.push('      show_help');
  lines.push('      exit 0');
  lines.push('      ;;');
  lines.push('    -d|--debug)');
  lines.push('      DEBUG_MODE=true');
  lines.push('      shift');
  lines.push('      ;;');
  lines.push('    -n|--no-net)');
  lines.push('      NO_NET=true');
  lines.push('      shift');
  lines.push('      ;;');
  lines.push('    --headless)');
  lines.push('      HEADLESS=true');
  lines.push('      shift');
  lines.push('      ;;');
  lines.push('    --)');
  lines.push('      shift');
  lines.push('      EXTRA_QEMU_ARGS+=("$@")');
  lines.push('      break');
  lines.push('      ;;');
  lines.push('    *)');
  lines.push('      EXTRA_QEMU_ARGS+=("$1")');
  lines.push('      shift');
  lines.push('      ;;');
  lines.push('  esac');
  lines.push('done');
  lines.push('');
  
  // File checks
  lines.push('# Check required files');
  lines.push('if [[ ! -f "$ISO_PATH" ]]; then');
  lines.push('  echo "ERROR: ISO image not found at $ISO_PATH" >&2');
  lines.push('  echo "Run: ndk full" >&2');
  lines.push('  exit 1');
  lines.push('fi');
  lines.push('');
  lines.push('if [[ ! -f "$ROOTFS_IMG" ]]; then');
  lines.push('  echo "ERROR: Root filesystem missing at $ROOTFS_IMG" >&2');
  lines.push('  echo "Run: ndk full" >&2');
  lines.push('  exit 1');
  lines.push('fi');
  lines.push('');
  lines.push('if [[ ! -f "$SWAP_IMG" ]]; then');
  lines.push('  echo "ERROR: Swap image missing at $SWAP_IMG" >&2');
  lines.push('  echo "Run: ndk full" >&2');
  lines.push('  exit 1');
  lines.push('fi');
  lines.push('');
  
  // Info output
  lines.push('echo "Starting NexaOS in QEMU..."');
  lines.push('echo "  Boot mode: ${BIOS_MODE^^}"');
  lines.push('echo "  Memory: $MEMORY"');
  lines.push('echo "  SMP cores: $SMP_CORES"');
  lines.push('echo "  Root device: $ROOTFS_IMG"');
  lines.push('echo "  Swap device: $SWAP_IMG"');
  lines.push('');
  
  // UEFI setup
  lines.push('# UEFI firmware setup');
  lines.push('UEFI_CODE=""');
  lines.push('UEFI_VARS_COPY=""');
  lines.push('');
  lines.push('if [[ "$BIOS_MODE" == "uefi" ]]; then');
  lines.push(`  CAND_DIRS=(${config.boot.uefi_paths.join(' ')})`);
  lines.push('');
  lines.push('  # Find UEFI code firmware');
  lines.push('  for d in "${CAND_DIRS[@]}"; do');
  lines.push('    for f in "$d"/OVMF_CODE*.fd; do');
  lines.push('      if [[ -f "$f" ]]; then');
  lines.push('        UEFI_CODE="$f"');
  lines.push('        break 2');
  lines.push('      fi');
  lines.push('    done');
  lines.push('  done');
  lines.push('');
  lines.push('  # Find UEFI vars template');
  lines.push('  UEFI_VARS_TEMPLATE=""');
  lines.push('  for d in "${CAND_DIRS[@]}"; do');
  lines.push('    for f in "$d"/OVMF_VARS*.fd; do');
  lines.push('      if [[ -f "$f" ]]; then');
  lines.push('        UEFI_VARS_TEMPLATE="$f"');
  lines.push('        break 2');
  lines.push('      fi');
  lines.push('    done');
  lines.push('  done');
  lines.push('');
  lines.push('  if [[ -z "$UEFI_CODE" || -z "$UEFI_VARS_TEMPLATE" ]]; then');
  lines.push('    echo "ERROR: OVMF firmware not found. Install edk2-ovmf package." >&2');
  lines.push('    exit 1');
  lines.push('  fi');
  lines.push('');
  lines.push('  UEFI_VARS_COPY="$ROOT_DIR/build/OVMF_VARS.fd"');
  lines.push('  mkdir -p "$ROOT_DIR/build"');
  lines.push('  if [[ ! -f "$UEFI_VARS_COPY" ]]; then');
  lines.push('    cp "$UEFI_VARS_TEMPLATE" "$UEFI_VARS_COPY"');
  lines.push('  fi');
  lines.push('fi');
  lines.push('');
  
  // Network setup
  if (config.network.enabled) {
    lines.push('# Network setup');
    lines.push('NET_MODE="user"');
    lines.push('NET_DEVICE=""');
    lines.push(`VM_MAC="${config.network.mac}"`);
    lines.push('');
    lines.push('if [[ "$NO_NET" != "true" ]]; then');
    
    if (config.network.mode === 'auto') {
      lines.push('  # Auto-detect network mode');
      lines.push('  DEFAULT_IF=$(ip route | grep default | awk \'{print $5}\' | head -n1)');
      lines.push('');
      lines.push('  if [[ -n "$DEFAULT_IF" ]]; then');
      lines.push('    if [[ -d "/sys/class/net/$DEFAULT_IF/wireless" ]] || iwconfig "$DEFAULT_IF" 2>/dev/null | grep -q "ESSID"; then');
      lines.push('      echo "  Network: user-mode (WiFi detected)"');
      lines.push('      NET_MODE="user"');
      lines.push('    else');
      lines.push('      echo "  Network: TAP bridge mode"');
      lines.push('      NET_MODE="tap"');
      lines.push(`      NET_DEVICE="${config.network.tap}"`);
      lines.push('');
      lines.push('      # Setup TAP device');
      lines.push('      sudo ip link delete $NET_DEVICE 2>/dev/null || true');
      lines.push('      sudo ip tuntap add dev $NET_DEVICE mode tap user "$(whoami)"');
      lines.push('      sudo ip link set $NET_DEVICE up promisc on');
      lines.push('');
      lines.push('      # Setup bridge if needed');
      lines.push(`      if ! ip link show ${config.network.bridge} &>/dev/null; then`);
      lines.push(`        sudo ip link add name ${config.network.bridge} type bridge`);
      lines.push(`        sudo ip link set ${config.network.bridge} up`);
      lines.push('        IP_ADDR=$(ip addr show "$DEFAULT_IF" | grep "inet " | awk \'{print $2}\')');
      lines.push('        if [[ -n "$IP_ADDR" ]]; then');
      lines.push('          sudo ip addr del "$IP_ADDR" dev "$DEFAULT_IF" 2>/dev/null || true');
      lines.push(`          sudo ip addr add "$IP_ADDR" dev ${config.network.bridge}`);
      lines.push('        fi');
      lines.push(`        sudo ip link set "$DEFAULT_IF" master ${config.network.bridge}`);
      lines.push('        GW=$(ip route | grep default | awk \'{print $3}\' | head -n1)');
      lines.push('        if [[ -n "$GW" ]]; then');
      lines.push('          sudo ip route del default 2>/dev/null || true');
      lines.push(`          sudo ip route add default via "$GW" dev ${config.network.bridge}`);
      lines.push('        fi');
      lines.push('      fi');
      lines.push(`      sudo ip link set $NET_DEVICE master ${config.network.bridge}`);
      lines.push('    fi');
      lines.push('  else');
      lines.push('    echo "  Network: user-mode (no default interface)"');
      lines.push('  fi');
    } else if (config.network.mode === 'user') {
      lines.push('  echo "  Network: user-mode"');
      lines.push('  NET_MODE="user"');
    } else if (config.network.mode === 'tap' || config.network.mode === 'bridge') {
      lines.push(`  NET_MODE="tap"`);
      lines.push(`  NET_DEVICE="${config.network.tap}"`);
      lines.push('  echo "  Network: TAP bridge mode"');
    }
    
    lines.push('else');
    lines.push('  echo "  Network: disabled"');
    lines.push('fi');
    lines.push('');
  }
  
  // Build QEMU command
  lines.push('# Build QEMU command');
  lines.push('QEMU_CMD=(');
  lines.push(`  qemu-system-${config.machine.arch}`);
  lines.push('  -m "$MEMORY"');
  lines.push('  -smp "$SMP_CORES"');
  lines.push(`  -serial ${config.serial.output}`);
  if (!config.serial.monitor) {
    lines.push('  -monitor none');
  }
  if (config.debug.guest_errors) {
    lines.push('  -d guest_errors');
  }
  lines.push(')');
  lines.push('');
  
  // Display (conditional)
  lines.push('# Display');
  lines.push('if [[ "$HEADLESS" == "true" ]]; then');
  lines.push('  QEMU_CMD+=(-display none)');
  lines.push('else');
  lines.push(`  QEMU_CMD+=(-vga ${config.display.vga})`);
  lines.push(`  QEMU_CMD+=(-display ${config.display.backend},window-close=${config.display.window_close ? 'on' : 'off'})`);
  lines.push('fi');
  lines.push('');
  
  // UEFI firmware
  lines.push('# Add UEFI firmware if needed');
  lines.push('if [[ "$BIOS_MODE" == "uefi" ]]; then');
  lines.push('  QEMU_CMD+=(');
  lines.push('    -drive if=pflash,format=raw,readonly=on,file="$UEFI_CODE"');
  lines.push('    -drive if=pflash,format=raw,file="$UEFI_VARS_COPY"');
  lines.push('  )');
  lines.push('fi');
  lines.push('');
  
  // Storage
  lines.push('# Storage devices');
  lines.push('QEMU_CMD+=(');
  lines.push('  -cdrom "$ISO_PATH"');
  
  // For IDE devices, we need to specify bus.unit to avoid conflicts
  // CDROM uses secondary master (ide.1), so we use primary channel for disks
  // Primary: ide.0 (master=unit 0, slave=unit 1)
  const rootfsDevice = config.storage.rootfs.device;
  const swapDevice = config.storage.swap.device;
  
  if (rootfsDevice === 'ide-hd') {
    // Use primary master (bus=ide.0, unit=0)
    lines.push(`  -drive file="$ROOTFS_IMG",id=rootfs,format=${config.storage.rootfs.format},if=none,cache=${config.storage.rootfs.cache}`);
    lines.push(`  -device ide-hd,drive=rootfs,bus=ide.0,unit=0`);
  } else {
    lines.push(`  -drive file="$ROOTFS_IMG",id=rootfs,format=${config.storage.rootfs.format},if=none,cache=${config.storage.rootfs.cache}`);
    lines.push(`  -device ${rootfsDevice},drive=rootfs`);
  }
  
  if (swapDevice === 'ide-hd') {
    // Use primary slave (bus=ide.0, unit=1)
    lines.push(`  -drive file="$SWAP_IMG",id=swap,format=${config.storage.swap.format},if=none,cache=${config.storage.swap.cache}`);
    lines.push(`  -device ide-hd,drive=swap,bus=ide.0,unit=1`);
  } else {
    lines.push(`  -drive file="$SWAP_IMG",id=swap,format=${config.storage.swap.format},if=none,cache=${config.storage.swap.cache}`);
    lines.push(`  -device ${swapDevice},drive=swap`);
  }
  lines.push(')');
  lines.push('');
  
  // Network
  if (config.network.enabled) {
    lines.push('# Network');
    lines.push('if [[ "$NO_NET" != "true" ]]; then');
    lines.push('  if [[ "$NET_MODE" == "user" ]]; then');
    
    // User mode with port forwarding
    let netdevStr = 'user,id=net0';
    if (config.network.user.hostfwd && config.network.user.hostfwd.length > 0) {
      for (const fwd of config.network.user.hostfwd) {
        const proto = fwd.includes('/') ? fwd.split('/')[1] : 'tcp';
        const cleanFwd = fwd.split('/')[0];
        netdevStr += `,hostfwd=${proto}::${cleanFwd.replace(':', '-')}`;
      }
    }
    lines.push(`    QEMU_CMD+=(-netdev "${netdevStr}")`);
    lines.push(`    QEMU_CMD+=(-device "${config.network.model},netdev=net0,mac=$VM_MAC")`);
    lines.push('  else');
    lines.push('    QEMU_CMD+=(-netdev "tap,id=net0,ifname=$NET_DEVICE,script=no,downscript=no")');
    lines.push(`    QEMU_CMD+=(-device "${config.network.model},netdev=net0,mac=$VM_MAC")`);
    lines.push('  fi');
    lines.push('fi');
    lines.push('');
  }
  
  // USB
  if (config.usb.enabled) {
    lines.push('# USB');
    lines.push(`QEMU_CMD+=(-usb -device ${config.usb.controller})`);
    for (const device of config.usb.devices) {
      lines.push(`QEMU_CMD+=(-device usb-${device})`);
    }
    lines.push('');
  }
  
  // Debug/GDB
  lines.push('# Debug mode');
  lines.push('if [[ "$DEBUG_MODE" == "true" ]]; then');
  lines.push(`  QEMU_CMD+=(-gdb tcp::${config.debug.gdb.port} -S)`);
  lines.push('  echo "  GDB server: localhost:${config.debug.gdb.port}"');
  lines.push('  echo "  CPU paused - waiting for GDB connection"');
  lines.push('fi');
  lines.push('');
  
  // Extra args
  lines.push('# Extra QEMU arguments');
  lines.push('if [[ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]]; then');
  lines.push('  echo "  Extra args: ${EXTRA_QEMU_ARGS[*]}"');
  lines.push('  QEMU_CMD+=("${EXTRA_QEMU_ARGS[@]}")');
  lines.push('fi');
  lines.push('');
  
  // Execute
  lines.push('echo ""');
  lines.push('exec "${QEMU_CMD[@]}"');
  
  return lines.join('\n');
}

/**
 * Clear cached configuration (useful for testing)
 */
export function clearQemuConfigCache(): void {
  cachedQemuConfig = null;
}

/**
 * Generate NEXA.CFG boot configuration file from config/qemu.yaml
 * This file is used by the UEFI loader to configure kernel boot parameters
 */
export async function generateNexaConfig(env: BuildEnvironment): Promise<string> {
  const config = await loadQemuConfig(env.projectRoot);
  const bootConfig = config.boot_config;
  
  const lines: string[] = [];
  
  // Header
  lines.push('# NexaOS Boot Configuration');
  lines.push('# Auto-generated from config/qemu.yaml by the build system');
  lines.push('# Do not edit directly - modify config/qemu.yaml instead');
  lines.push('');
  
  // Root device
  lines.push(`# Root device - the block device containing the root filesystem`);
  lines.push(`root=${bootConfig.root}`);
  lines.push('');
  
  // Root filesystem type
  lines.push(`# Root filesystem type`);
  lines.push(`rootfstype=${bootConfig.rootfstype}`);
  lines.push('');
  
  // Init program
  lines.push(`# Init program path - first userspace process`);
  lines.push(`init=${bootConfig.init}`);
  lines.push('');
  
  // Serial console (optional)
  if (bootConfig.console) {
    lines.push(`# Serial console configuration`);
    lines.push(`console=${bootConfig.console}`);
    lines.push('');
  }
  
  // Quiet mode (optional)
  if (bootConfig.quiet) {
    lines.push(`# Quiet boot mode enabled`);
    lines.push(`quiet`);
    lines.push('');
  }
  
  // Debug mode (optional)
  if (bootConfig.debug) {
    lines.push(`# Debug mode enabled`);
    lines.push(`debug`);
    lines.push('');
  }
  
  const content = lines.join('\n');
  
  // Write to build directory
  const outputPath = join(env.buildDir, 'NEXA.CFG');
  await mkdir(env.buildDir, { recursive: true });
  await writeFile(outputPath, content, 'utf-8');
  
  logger.success(`Generated ${outputPath}`);
  
  return outputPath;
}
