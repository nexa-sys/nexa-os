import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import { api } from '@/api';

export interface Feature {
  enabled: boolean;
  description: string;
  cfg_flag: string;
  dependencies: string[];
  required?: boolean;
}

export interface FeatureCategory {
  [key: string]: Feature;
}

export interface FeaturesConfig {
  network?: FeatureCategory;
  kernel?: FeatureCategory;
  filesystem?: FeatureCategory;
  security?: FeatureCategory;
  graphics?: FeatureCategory;
}

export interface ModulesConfig {
  modules: Record<string, { enabled: boolean }>;
}

export interface ProgramConfig {
  package: string;
  path: string;
  description: string;
  dest: string;
  enabled: boolean;
  required?: boolean;
}

export interface ProgramsConfig {
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
}

export interface Estimate {
  features: { enabled: number; total: number };
  modules: { enabled: number; total: number };
  programs: { enabled: number; total: number };
  estimatedSize: number;
  estimatedSizeMB: string;
}

export const useConfigStore = defineStore('config', () => {
  const features = ref<FeaturesConfig | null>(null);
  const modules = ref<Record<string, { enabled: boolean }>>({});
  const programs = ref<ProgramsConfig | null>(null);
  const estimate = ref<Estimate | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const isLoading = computed(() => loading.value);

  async function loadConfig() {
    loading.value = true;
    error.value = null;
    try {
      const [featuresRes, modulesRes, programsRes, estimateRes] = await Promise.all([
        api.getFeatures(),
        api.getModules(),
        api.getPrograms(),
        api.getEstimate()
      ]);
      features.value = featuresRes;
      modules.value = modulesRes.modules || {};
      programs.value = programsRes;
      estimate.value = estimateRes;
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to load configuration';
    } finally {
      loading.value = false;
    }
  }

  async function toggleFeature(category: string, name: string) {
    if (features.value) {
      const cat = features.value[category as keyof FeaturesConfig];
      if (cat && cat[name]) {
        const feature = cat[name];
        if (feature.required) return;
        
        const newEnabled = !feature.enabled;
        try {
          await api.updateFeature(category, name, newEnabled);
          feature.enabled = newEnabled;
          estimate.value = await api.getEstimate();
        } catch (e) {
          error.value = e instanceof Error ? e.message : 'Failed to update feature';
        }
      }
    }
  }

  async function toggleModule(name: string) {
    if (modules.value[name]) {
      const newEnabled = !modules.value[name].enabled;
      try {
        await api.updateModule(name, newEnabled);
        modules.value[name].enabled = newEnabled;
        estimate.value = await api.getEstimate();
      } catch (e) {
        error.value = e instanceof Error ? e.message : 'Failed to update module';
      }
    }
  }

  async function toggleProgram(category: string, pkg: string) {
    if (programs.value) {
      const cat = programs.value[category as keyof ProgramsConfig];
      if (cat) {
        const program = cat.find((p: ProgramConfig) => p.package === pkg);
        if (program && !program.required) {
          const newEnabled = !program.enabled;
          try {
            await api.updateProgram(category, pkg, newEnabled);
            program.enabled = newEnabled;
            estimate.value = await api.getEstimate();
          } catch (e) {
            error.value = e instanceof Error ? e.message : 'Failed to update program';
          }
        }
      }
    }
  }

  async function applyPreset(type: 'features' | 'modules', presetName: string) {
    loading.value = true;
    try {
      await api.applyPreset(type, presetName);
      await loadConfig();
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to apply preset';
    } finally {
      loading.value = false;
    }
  }

  return {
    features,
    modules,
    programs,
    estimate,
    loading,
    error,
    isLoading,
    loadConfig,
    toggleFeature,
    toggleModule,
    toggleProgram,
    applyPreset
  };
});
