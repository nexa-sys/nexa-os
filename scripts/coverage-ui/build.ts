/**
 * Build script for coverage UI
 * 
 * This script builds the Vue app and creates a single HTML file
 * that can be generated with embedded coverage data.
 */

import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const distDir = resolve(__dirname, 'dist');

/**
 * Generate a single HTML file with embedded coverage data
 */
export function generateCoverageHtml(coverageData: object): string {
  // Read the built template
  const templatePath = resolve(__dirname, 'dist', 'index.html');
  
  if (!existsSync(templatePath)) {
    throw new Error('Coverage UI not built. Run: cd scripts/coverage-ui && npm install && npm run build');
  }
  
  let html = readFileSync(templatePath, 'utf-8');
  
  // Inject coverage data
  const dataScript = `<script id="coverage-data" type="application/json">${JSON.stringify(coverageData)}</script>`;
  html = html.replace('</head>', `${dataScript}\n</head>`);
  
  return html;
}

/**
 * Inline all assets into the HTML file
 */
export function inlineAssets(html: string): string {
  // The Vite build should already inline everything with the config we set
  return html;
}
