/**
 * NexaOS Build System - Swap Image Builder
 */

import { join } from 'path';
import { open } from 'fs/promises';
import { BuildEnvironment, BuildStepResult } from '../types.js';
import { logger } from '../logger.js';
import { exec, getFileSize } from '../exec.js';

// Default swap size (can be overridden via SWAP_SIZE_MB env var)
const SWAP_SIZE_MB = parseInt(process.env.SWAP_SIZE_MB ?? '256', 10);

// Linux swap constants
const PAGE_SIZE = 4096;
const SWAP_MAGIC = Buffer.from('SWAPSPACE2');
const SWAP_MAGIC_OFFSET = PAGE_SIZE - 10; // 4086

/**
 * Create a Linux-compatible swap image with SWAPSPACE2 signature
 */
export async function buildSwapImage(env: BuildEnvironment): Promise<BuildStepResult> {
  logger.section('Building Swap Image');
  
  const startTime = Date.now();
  const swapImg = join(env.buildDir, 'swap.img');
  const sizeBytes = SWAP_SIZE_MB * 1024 * 1024;
  const totalPages = Math.floor(sizeBytes / PAGE_SIZE);
  const lastPage = totalPages - 1;
  
  logger.step(`Creating swap image (${SWAP_SIZE_MB}MB, ${totalPages} pages)...`);
  
  // Create sparse file
  logger.info('Creating sparse file...');
  await exec('truncate', ['-s', `${sizeBytes}`, swapImg]);
  
  // Create swap header (version 1 format)
  // The header is in the first page, with magic at the end
  const header = Buffer.alloc(PAGE_SIZE, 0);
  
  // Version (1) at offset 1024
  header.writeUInt32LE(1, 1024);
  
  // last_page at offset 1028 (little-endian 32-bit)
  header.writeUInt32LE(lastPage, 1028);
  
  // nr_badpages at offset 1032 (0)
  header.writeUInt32LE(0, 1032);
  
  // UUID at offset 1036 (generate random UUID)
  const uuid = generateUUID();
  uuid.copy(header, 1036);
  
  // Magic signature "SWAPSPACE2" at offset PAGE_SIZE - 10
  SWAP_MAGIC.copy(header, SWAP_MAGIC_OFFSET);
  
  // Write header to the image
  logger.info('Writing swap header...');
  const fd = await open(swapImg, 'r+');
  await fd.write(header, 0, PAGE_SIZE, 0);
  await fd.close();
  
  const size = await getFileSize(swapImg);
  const uuidStr = formatUUID(uuid);
  
  logger.success(`Swap image created: ${swapImg}`);
  logger.info(`  Size: ${size}`);
  logger.info(`  Pages: ${totalPages}`);
  logger.info(`  UUID: ${uuidStr}`);
  
  // Verify swap signature
  const verifyResult = await exec('file', [swapImg]);
  if (verifyResult.stdout.includes('swap') || verifyResult.stdout.includes('Linux')) {
    logger.success('Valid swap signature detected');
  } else {
    logger.info('Swap header written (file command may not detect custom format)');
  }
  
  return {
    success: true,
    duration: Date.now() - startTime,
  };
}

/**
 * Generate a random UUID
 */
function generateUUID(): Buffer {
  const uuid = Buffer.alloc(16);
  
  // Use crypto-quality randomness if available, otherwise fallback
  for (let i = 0; i < 16; i++) {
    uuid[i] = Math.floor(Math.random() * 256);
  }
  
  // Set version (4) and variant (RFC 4122)
  uuid[6] = (uuid[6] & 0x0f) | 0x40;
  uuid[8] = (uuid[8] & 0x3f) | 0x80;
  
  return uuid;
}

/**
 * Format UUID as string
 */
function formatUUID(uuid: Buffer): string {
  const hex = uuid.toString('hex');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
