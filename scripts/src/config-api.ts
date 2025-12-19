/**
 * NexaOS Configuration API Server
 * Express-based HTTP API for the configuration UI
 */

import express, { Request, Response } from 'express';
import cors from 'cors';
import { readFileSync, writeFileSync } from 'fs';
import { resolve } from 'path';
import YAML from 'yaml';
import { logger } from './logger.js';

// Types
interface Feature {
  enabled: boolean;
  description: string;
  cfg_flag: string;
  dependencies: string[];
  required?: boolean;
}

interface FeatureCategory {
  [key: string]: Feature;
}

interface FeaturesConfig {
  network?: FeatureCategory;
  kernel?: FeatureCategory;
  filesystem?: FeatureCategory;
  security?: FeatureCategory;
  graphics?: FeatureCategory;
  presets?: Record<string, {
    description: string;
    features: Record<string, boolean>;
  }>;
}

interface ModuleConfig {
  enabled: boolean;
}

interface ModulesConfig {
  modules: Record<string, ModuleConfig>;
  autoload?: {
    initramfs: string[];
    rootfs: string[];
  };
  signing?: {
    required: boolean;
    certificate: string;
    private_key: string;
    algorithm: string;
  };
  profiles?: Record<string, {
    enabled?: string[];
    disabled?: string[];
  }>;
}

interface ProgramConfig {
  package: string;
  path: string;
  description: string;
  dest: string;
  link?: string;
  enabled: boolean;
  required?: boolean;
  features?: string;
}

interface ProgramsConfig {
  core?: ProgramConfig[];
  user?: ProgramConfig[];
  network?: ProgramConfig[];
  daemons?: ProgramConfig[];
  system?: ProgramConfig[];
  coreutils?: ProgramConfig[];
  power?: ProgramConfig[];
  memory?: ProgramConfig[];
  ipc?: ProgramConfig[];
  editors?: ProgramConfig[];
  kmod?: ProgramConfig[];
  test?: ProgramConfig[];
}

// Helper functions
function loadYaml<T>(configDir: string, filename: string): T {
  const filepath = resolve(configDir, filename);
  const content = readFileSync(filepath, 'utf-8');
  return YAML.parse(content) as T;
}

function saveYaml<T>(configDir: string, filename: string, data: T): void {
  const filepath = resolve(configDir, filename);
  const content = YAML.stringify(data, { lineWidth: 0 });
  writeFileSync(filepath, content, 'utf-8');
}

export function startConfigApi(projectRoot: string, port: number): Promise<void> {
  const configDir = resolve(projectRoot, 'config');
  const app = express();

  app.use(cors());
  app.use(express.json());

  // GET /api/features
  app.get('/api/features', (_req: Request, res: Response) => {
    try {
      const features = loadYaml<FeaturesConfig>(configDir, 'features.yaml');
      res.json(features);
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // PUT /api/features/:category/:name
  app.put('/api/features/:category/:name', (req: Request, res: Response) => {
    try {
      const { category, name } = req.params;
      const { enabled } = req.body;

      const features = loadYaml<FeaturesConfig>(configDir, 'features.yaml');
      const cat = features[category as keyof FeaturesConfig] as FeatureCategory | undefined;

      if (!cat || !cat[name]) {
        res.status(404).json({ error: 'Feature not found' });
        return;
      }

      if (cat[name].required && !enabled) {
        res.status(400).json({ error: 'Cannot disable required feature' });
        return;
      }

      cat[name].enabled = enabled;
      saveYaml(configDir, 'features.yaml', features);

      res.json({ success: true, feature: cat[name] });
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // GET /api/modules
  app.get('/api/modules', (_req: Request, res: Response) => {
    try {
      const modules = loadYaml<ModulesConfig>(configDir, 'modules.yaml');
      res.json(modules);
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // PUT /api/modules/:name
  app.put('/api/modules/:name', (req: Request, res: Response) => {
    try {
      const { name } = req.params;
      const { enabled } = req.body;

      const config = loadYaml<ModulesConfig>(configDir, 'modules.yaml');

      if (!config.modules[name]) {
        res.status(404).json({ error: 'Module not found' });
        return;
      }

      config.modules[name].enabled = enabled;
      saveYaml(configDir, 'modules.yaml', config);

      res.json({ success: true, module: config.modules[name] });
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // GET /api/programs
  app.get('/api/programs', (_req: Request, res: Response) => {
    try {
      const programs = loadYaml<ProgramsConfig>(configDir, 'programs.yaml');
      res.json(programs);
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // PUT /api/programs/:category/:pkg
  app.put('/api/programs/:category/:pkg', (req: Request, res: Response) => {
    try {
      const { category, pkg } = req.params;
      const { enabled } = req.body;

      const programs = loadYaml<ProgramsConfig>(configDir, 'programs.yaml');
      const cat = programs[category as keyof ProgramsConfig];

      if (!cat) {
        res.status(404).json({ error: 'Category not found' });
        return;
      }

      const program = cat.find((p: ProgramConfig) => p.package === pkg);
      if (!program) {
        res.status(404).json({ error: 'Program not found' });
        return;
      }

      if (program.required && !enabled) {
        res.status(400).json({ error: 'Cannot disable required program' });
        return;
      }

      program.enabled = enabled;
      saveYaml(configDir, 'programs.yaml', programs);

      res.json({ success: true, program });
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // GET /api/presets
  app.get('/api/presets', (_req: Request, res: Response) => {
    try {
      const features = loadYaml<FeaturesConfig>(configDir, 'features.yaml');
      const modules = loadYaml<ModulesConfig>(configDir, 'modules.yaml');

      res.json({
        features: features.presets || {},
        modules: modules.profiles || {}
      });
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // POST /api/presets/apply
  app.post('/api/presets/apply', (req: Request, res: Response) => {
    try {
      const { type, name } = req.body;

      if (type === 'features') {
        const features = loadYaml<FeaturesConfig>(configDir, 'features.yaml');
        const preset = features.presets?.[name];

        if (!preset) {
          res.status(404).json({ error: 'Preset not found' });
          return;
        }

        for (const [featurePath, enabled] of Object.entries(preset.features)) {
          const [cat, featureName] = featurePath.split('.');
          const category = features[cat as keyof FeaturesConfig] as FeatureCategory | undefined;
          if (category && category[featureName] && !category[featureName].required) {
            category[featureName].enabled = enabled;
          }
        }

        saveYaml(configDir, 'features.yaml', features);
      } else if (type === 'modules') {
        const modules = loadYaml<ModulesConfig>(configDir, 'modules.yaml');
        const profile = modules.profiles?.[name];

        if (!profile) {
          res.status(404).json({ error: 'Profile not found' });
          return;
        }

        for (const mod of Object.keys(modules.modules)) {
          modules.modules[mod].enabled = false;
        }

        for (const mod of profile.enabled || []) {
          if (modules.modules[mod]) {
            modules.modules[mod].enabled = true;
          }
        }

        saveYaml(configDir, 'modules.yaml', modules);
      }

      res.json({ success: true });
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // GET /api/estimate
  app.get('/api/estimate', (_req: Request, res: Response) => {
    try {
      const features = loadYaml<FeaturesConfig>(configDir, 'features.yaml');
      const modules = loadYaml<ModulesConfig>(configDir, 'modules.yaml');
      const programs = loadYaml<ProgramsConfig>(configDir, 'programs.yaml');

      let enabledFeatures = 0;
      let totalFeatures = 0;

      for (const category of ['network', 'kernel', 'filesystem', 'security', 'graphics'] as const) {
        const cat = features[category];
        if (cat) {
          for (const feature of Object.values(cat)) {
            totalFeatures++;
            if (feature.enabled) enabledFeatures++;
          }
        }
      }

      let enabledModules = 0;
      const totalModules = Object.keys(modules.modules).length;
      for (const mod of Object.values(modules.modules)) {
        if (mod.enabled) enabledModules++;
      }

      let enabledPrograms = 0;
      let totalPrograms = 0;
      for (const cat of Object.values(programs)) {
        if (Array.isArray(cat)) {
          for (const prog of cat) {
            totalPrograms++;
            if (prog.enabled) enabledPrograms++;
          }
        }
      }

      const baseKernelSize = 2 * 1024 * 1024;
      const featureSize = enabledFeatures * 50 * 1024;
      const moduleSize = enabledModules * 100 * 1024;
      const programSize = enabledPrograms * 30 * 1024;

      const estimatedSize = baseKernelSize + featureSize + moduleSize + programSize;

      res.json({
        features: { enabled: enabledFeatures, total: totalFeatures },
        modules: { enabled: enabledModules, total: totalModules },
        programs: { enabled: enabledPrograms, total: totalPrograms },
        estimatedSize,
        estimatedSizeMB: (estimatedSize / (1024 * 1024)).toFixed(2)
      });
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // Start server
  return new Promise((resolvePromise, reject) => {
    const server = app.listen(port, '127.0.0.1', () => {
      logger.success(`Config API server running at http://127.0.0.1:${port}`);
      resolvePromise();
    });

    server.on('error', (err: NodeJS.ErrnoException) => {
      if (err.code === 'EADDRINUSE') {
        logger.error(`Port ${port} is already in use`);
      }
      reject(err);
    });
  });
}
