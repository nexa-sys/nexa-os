/**
 * NexaOS Build System - ISO Builder
 */

import { join } from 'path';
import { mkdir, copyFile, writeFile, rm } from 'fs/promises';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { exec, requireCommands, getFileSize } from '../exec.js';

/**
 * Generate GRUB configuration
 */
function generateGrubConfig(env: BuildEnvironment, hasInitramfs: boolean, hasUefi: boolean): string {
  const grubCmdline = `root=/dev/vda1 rootfstype=ext2 loglevel=${env.logLevel}`;
  
  let config = `set timeout=3
set default=0

# Detect UEFI environment
if [ "$grub_platform" = "efi" ]; then
    if loadfont /boot/grub/fonts/unicode.pf2; then
        set gfxmode=auto
        insmod efi_gop
        insmod efi_uga
        insmod gfxterm
        terminal_output gfxterm
    else
        terminal_output console
    fi
`;

  if (hasUefi) {
    config += `
    # UEFI boot entry
    menuentry "NexaOS (UEFI)" {
        insmod part_msdos
        insmod ext2
        echo 'Loading NexaOS UEFI Loader...'
        chainloader /EFI/BOOT/BOOTX64.EFI
    }
`;
  }

  config += `else
    terminal_output console
fi

set gfxpayload=keep
insmod video_bochs
insmod video_cirrus

# Legacy BIOS boot entry
menuentry "NexaOS (Legacy)" {
    multiboot2 /boot/kernel.elf ${grubCmdline}
`;

  if (hasInitramfs) {
    config += `    module2 /boot/initramfs.cpio
`;
  }

  config += `    boot
}

# Serial console boot entry
menuentry "NexaOS (Serial Console)" {
    multiboot2 /boot/kernel.elf ${grubCmdline} console=ttyS0
`;

  if (hasInitramfs) {
    config += `    module2 /boot/initramfs.cpio
`;
  }

  config += `    boot
}
`;

  return config;
}

/**
 * Build bootable ISO image
 */
export async function buildIso(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.section('Building Bootable ISO');
  
  const startTime = Date.now();
  
  // Check dependencies
  const missing = await requireCommands(['grub-mkrescue', 'xorriso']);
  if (missing.length > 0) {
    logger.error(`Missing required tools: ${missing.join(', ')}`);
    return { success: false, duration: 0, error: `Missing tools: ${missing.join(', ')}` };
  }
  
  const isoWorkDir = join(env.targetDir, 'iso');
  
  // Clean and setup ISO structure
  logger.step('Setting up ISO structure...');
  await rm(isoWorkDir, { recursive: true, force: true });
  await mkdir(join(isoWorkDir, 'boot/grub'), { recursive: true });
  await mkdir(env.distDir, { recursive: true });
  
  // Copy kernel
  if (!existsSync(env.kernelBin)) {
    logger.error('Kernel binary not found. Build kernel first.');
    return { success: false, duration: 0, error: 'Kernel not found' };
  }
  
  await copyFile(env.kernelBin, join(isoWorkDir, 'boot/kernel.elf'));
  const kernelSize = await getFileSize(env.kernelBin);
  logger.info(`Kernel: ${kernelSize}`);
  
  // Copy UEFI loader if available
  let hasUefi = false;
  const uefiLoader = join(env.buildDir, 'BootX64.EFI');
  if (existsSync(uefiLoader)) {
    logger.step('Adding UEFI loader...');
    await mkdir(join(isoWorkDir, 'EFI/BOOT'), { recursive: true });
    await copyFile(uefiLoader, join(isoWorkDir, 'EFI/BOOT/BOOTX64.EFI'));
    await copyFile(env.kernelBin, join(isoWorkDir, 'EFI/BOOT/KERNEL.ELF'));
    await copyFile(env.kernelBin, join(isoWorkDir, 'boot/KERNEL.ELF'));
    hasUefi = true;
    logger.success('UEFI loader included');
  }
  
  // Copy GRUB font
  logger.step('Copying GRUB font...');
  const fontCandidates = [
    '/usr/share/grub/unicode.pf2',
    '/usr/share/grub2/unicode.pf2',
  ];
  
  for (const fontPath of fontCandidates) {
    if (existsSync(fontPath)) {
      await mkdir(join(isoWorkDir, 'boot/grub/fonts'), { recursive: true });
      await copyFile(fontPath, join(isoWorkDir, 'boot/grub/fonts/unicode.pf2'));
      logger.success('GRUB font installed');
      break;
    }
  }
  
  // Copy initramfs if available
  let hasInitramfs = false;
  if (existsSync(env.initramfsCpio)) {
    logger.step('Adding initramfs...');
    await copyFile(env.initramfsCpio, join(isoWorkDir, 'boot/initramfs.cpio'));
    
    if (hasUefi) {
      await copyFile(env.initramfsCpio, join(isoWorkDir, 'EFI/BOOT/INITRAMFS.CPIO'));
      await copyFile(env.initramfsCpio, join(isoWorkDir, 'boot/INITRAMFS.CPIO'));
    }
    
    const initramfsSize = await getFileSize(env.initramfsCpio);
    logger.success(`Initramfs included (${initramfsSize})`);
    hasInitramfs = true;
  }
  
  // Generate GRUB config
  logger.step('Generating GRUB configuration...');
  const grubConfig = generateGrubConfig(env, hasInitramfs, hasUefi);
  await writeFile(join(isoWorkDir, 'boot/grub/grub.cfg'), grubConfig);
  
  // Build ISO
  logger.step('Creating ISO image...');
  
  const grubArgs = [
    '-o', env.isoFile,
    isoWorkDir,
  ];
  
  const result = await exec('grub-mkrescue', grubArgs);
  
  if (result.exitCode !== 0) {
    logger.error('Failed to create ISO');
    console.error(result.stderr);
    return { success: false, duration: Date.now() - startTime, error: result.stderr };
  }
  
  const isoSize = await getFileSize(env.isoFile);
  logger.success(`ISO created: ${env.isoFile} (${isoSize})`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}
