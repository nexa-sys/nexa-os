/**
 * Build script for docs UI
 * 
 * This script builds the Vue app and creates a single HTML file
 * that can be generated with embedded documentation data.
 */

import { readFileSync, existsSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));

export interface DocEntry {
  name: string;
  path: string;
  description: string;
  category: 'core' | 'drivers' | 'userspace' | 'tools';
}

export interface DocsData {
  projectName: string;
  version: string;
  timestamp: string;
  docs: DocEntry[];
  workspaces: string[];
}

/**
 * Generate a single HTML file with embedded documentation data
 */
export function generateDocsHtml(docsData: DocsData): string {
  // Read the built template
  const templatePath = resolve(__dirname, 'dist', 'index.html');
  
  if (!existsSync(templatePath)) {
    throw new Error('Docs UI not built. Run: cd scripts/ui/docs && npm install && npm run build');
  }
  
  let html = readFileSync(templatePath, 'utf-8');
  
  // Inject docs data
  const dataScript = `<script id="docs-data" type="application/json">${JSON.stringify(docsData)}</script>`;
  html = html.replace('</head>', `${dataScript}\n</head>`);
  
  return html;
}
