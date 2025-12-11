/**
 * NexaOS Build System - ISO Builder
 */

import { join } from 'path';
import { mkdir, copyFile, writeFile, rm, stat } from 'fs/promises';
import { existsSync } from 'fs';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { exec, requireCommands, getFileSize } from '../exec.js';
import { glob } from 'glob';
import { generateNexaConfig } from '../qemu.js';

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
    
    // Generate and copy NEXA.CFG boot configuration from config/qemu.yaml
    try {
      const nexaCfgPath = await generateNexaConfig(env);
      await copyFile(nexaCfgPath, join(isoWorkDir, 'EFI/BOOT/NEXA.CFG'));
      await copyFile(nexaCfgPath, join(isoWorkDir, 'NEXA.CFG'));
      logger.success('NEXA.CFG boot configuration generated and included');
    } catch (err) {
      logger.warn(`Failed to generate NEXA.CFG: ${err}, UEFI loader will use defaults`);
    }
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
  
  // Post-process ESP for UEFI boot
  if (hasUefi) {
    const espResult = await postprocessEsp(env);
    if (!espResult) {
      logger.warn('ESP post-processing failed, UEFI boot may not work');
    }
  }
  
  const isoSize = await getFileSize(env.isoFile);
  logger.success(`ISO created: ${env.isoFile} (${isoSize})`);
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Post-process the EFI System Partition in the ISO
 * This is required because grub-mkrescue creates a small ESP that doesn't include our files
 */
async function postprocessEsp(env: BuildEnvironment): Promise<boolean> {
  // Check for required tools
  const missing = await requireCommands(['7z', 'mkfs.vfat', 'mmd', 'mcopy', 'mdir']);
  if (missing.length > 0) {
    logger.warn(`Missing tools for ESP modification: ${missing.join(', ')}`);
    logger.info('Install mtools and p7zip-full for UEFI boot support');
    return false;
  }
  
  logger.step('Post-processing ESP for UEFI boot...');
  
  const isoTemp = join(env.buildDir, `iso_extract_${process.pid}`);
  await rm(isoTemp, { recursive: true, force: true });
  await mkdir(isoTemp, { recursive: true });
  
  try {
    // Extract ISO
    logger.info('Extracting ISO...');
    await exec('7z', ['x', `-o${isoTemp}`, env.isoFile, '-bsp0', '-bso0']);
    
    // Find efi.img
    const efiImgCandidates = await glob('**/efi.img', { cwd: isoTemp, absolute: true });
    if (efiImgCandidates.length === 0) {
      logger.warn('Could not find efi.img in ISO');
      return false;
    }
    
    const efiImg = efiImgCandidates[0];
    logger.info(`Found ESP: ${efiImg}`);
    
    // Calculate required ESP size
    const kernelStat = await stat(env.kernelBin);
    const bootloaderStat = await stat(join(env.buildDir, 'BootX64.EFI'));
    let initramfsStat = { size: 0 };
    if (existsSync(env.initramfsCpio)) {
      initramfsStat = await stat(env.initramfsCpio);
    }
    let nexaCfgStat = { size: 0 };
    const nexaCfgPath = join(env.projectRoot, 'boot/NEXA.CFG');
    if (existsSync(nexaCfgPath)) {
      nexaCfgStat = await stat(nexaCfgPath);
    }
    
    const totalBytes = kernelStat.size + bootloaderStat.size + initramfsStat.size + nexaCfgStat.size;
    let espSizeMb = Math.ceil((totalBytes * 1.2) / (1024 * 1024));
    if (espSizeMb < 16) espSizeMb = 16;
    
    logger.info(`Creating ${espSizeMb}MB ESP...`);
    
    const newEsp = join(env.buildDir, 'new_efi.img');
    
    // Create new ESP image
    await exec('dd', ['if=/dev/zero', `of=${newEsp}`, 'bs=1M', `count=${espSizeMb}`, 'status=none']);
    await exec('mkfs.vfat', ['-F', '12', '-n', 'UEFI', newEsp]);
    
    // Create directories
    await exec('mmd', ['-i', newEsp, '::/EFI']).catch(() => {});
    await exec('mmd', ['-i', newEsp, '::/EFI/BOOT']).catch(() => {});
    
    // Copy files to ESP
    await exec('mcopy', ['-i', newEsp, join(env.buildDir, 'BootX64.EFI'), '::/EFI/BOOT/BOOTX64.EFI']);
    await exec('mcopy', ['-i', newEsp, env.kernelBin, '::/EFI/BOOT/KERNEL.ELF']);
    
    if (existsSync(env.initramfsCpio)) {
      await exec('mcopy', ['-i', newEsp, env.initramfsCpio, '::/EFI/BOOT/INITRAMFS.CPIO']);
    }
    
    // Copy NEXA.CFG boot configuration to ESP
    if (existsSync(nexaCfgPath)) {
      await exec('mcopy', ['-i', newEsp, nexaCfgPath, '::/EFI/BOOT/NEXA.CFG']);
      logger.info('NEXA.CFG copied to ESP');
    }
    
    // Replace ESP in extracted ISO
    await exec('chmod', ['u+w', efiImg]);
    await copyFile(newEsp, efiImg);
    await rm(newEsp, { force: true });
    
    // Verify ESP contents
    logger.info('ESP contents:');
    const mdirResult = await exec('mdir', ['-i', efiImg, '::/EFI/BOOT/']);
    console.log(mdirResult.stdout);
    
    // Rebuild ISO
    logger.info('Rebuilding ISO with modified ESP...');
    await rm(env.isoFile, { force: true });
    
    const rebuildResult = await exec('grub-mkrescue', ['-o', env.isoFile, isoTemp]);
    if (rebuildResult.exitCode !== 0) {
      logger.error('Failed to rebuild ISO');
      return false;
    }
    
    logger.success('ESP modified successfully');
    return true;
    
  } finally {
    // Cleanup
    await rm(isoTemp, { recursive: true, force: true });
  }
}
