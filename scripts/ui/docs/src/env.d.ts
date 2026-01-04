/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

interface DocEntry {
  name: string;
  path: string;
  description: string;
  category: 'core' | 'drivers' | 'userspace' | 'tools';
}

interface DocsData {
  projectName: string;
  version: string;
  timestamp: string;
  docs: DocEntry[];
  workspaces: string[];
}
