/**
 * NexaOS Build System - Feature Flags Management
 * CLI commands for managing kernel compile-time features
 */

import { readFile, writeFile } from 'fs/promises';
import { join } from 'path';
import { parse as parseYaml, stringify as stringifyYaml } from 'yaml';
import chalk from 'chalk';
import { BuildEnvironment } from './types.js';
import { FeatureFlagsConfig, FeatureDefinition } from './types.js';

const FEATURE_CATEGORIES = ['network', 'kernel', 'filesystem', 'security', 'graphics', 'debug'] as const;
type FeatureCategory = typeof FEATURE_CATEGORIES[number];

/**
 * Load features.yaml configuration
 */
export async function loadFeaturesConfig(projectRoot: string): Promise<FeatureFlagsConfig> {
  const featuresPath = join(projectRoot, 'config', 'features.yaml');
  const content = await readFile(featuresPath, 'utf-8');
  return parseYaml(content) as FeatureFlagsConfig;
}

/**
 * Save features.yaml configuration
 */
export async function saveFeaturesConfig(projectRoot: string, config: FeatureFlagsConfig): Promise<void> {
  const featuresPath = join(projectRoot, 'config', 'features.yaml');
  
  // Preserve the header comments
  const header = `# NexaOS Feature Flags Configuration
#
# This file controls compile-time feature flags for the kernel.
# Features can be enabled/disabled here and will be passed to Cargo as --features.
# Use FEATURE_xxx environment variables to override at build time.
#
# Example: FEATURE_TCP=false ./scripts/build.sh kernel
#          FEATURE_TTF=false ./scripts/build.sh kernel      # Disable TTF fonts
#          FEATURE_COMPOSITOR=false ./scripts/build.sh kernel # Disable compositor
#          FEATURE_NUMA=false ./scripts/build.sh kernel     # Disable NUMA support
#
# NOTE: Disabling core networking features may break userspace programs that depend on them.
# NOTE: Disabling TTF falls back to 8x8 bitmap font (ASCII only).
# NOTE: Disabling compositor disables parallel rendering optimizations.

`;
  
  const yamlContent = stringifyYaml(config, { lineWidth: 120 });
  await writeFile(featuresPath, header + yamlContent, 'utf-8');
}

/**
 * List all features with their status
 */
export async function listFeatures(env: BuildEnvironment, options: {
  category?: string;
  enabled?: boolean;
  disabled?: boolean;
  verbose?: boolean;
}): Promise<void> {
  const config = await loadFeaturesConfig(env.projectRoot);
  
  console.log(chalk.bold.cyan('\n📦 NexaOS Kernel Features\n'));
  
  const categories = options.category 
    ? [options.category as FeatureCategory]
    : FEATURE_CATEGORIES;
  
  for (const category of categories) {
    const categoryConfig = config[category];
    if (!categoryConfig) continue;
    
    const features = Object.entries(categoryConfig as Record<string, FeatureDefinition>);
    if (features.length === 0) continue;
    
    // Filter by enabled/disabled if specified
    const filteredFeatures = features.filter(([_, feature]) => {
      if (options.enabled && !feature.enabled) return false;
      if (options.disabled && feature.enabled) return false;
      return true;
    });
    
    if (filteredFeatures.length === 0) continue;
    
    console.log(chalk.bold.yellow(`\n[${category.toUpperCase()}]`));
    
    for (const [name, feature] of filteredFeatures) {
      const status = feature.enabled 
        ? chalk.green('✓ enabled') 
        : chalk.red('✗ disabled');
      
      const required = feature.required ? chalk.magenta(' (required)') : '';
      const deps = feature.dependencies?.length 
        ? chalk.gray(` → depends: [${feature.dependencies.join(', ')}]`)
        : '';
      
      console.log(`  ${chalk.white(name.padEnd(20))} ${status}${required}${deps}`);
      
      if (options.verbose) {
        console.log(chalk.gray(`    ${feature.description}`));
        console.log(chalk.gray(`    cfg_flag: ${feature.cfg_flag}`));
      }
    }
  }
  
  console.log('');
}

/**
 * List all available presets
 */
export async function listPresets(env: BuildEnvironment, verbose: boolean = false): Promise<void> {
  const config = await loadFeaturesConfig(env.projectRoot);
  
  console.log(chalk.bold.cyan('\n🎯 Feature Presets\n'));
  
  if (!config.presets) {
    console.log(chalk.yellow('  No presets defined'));
    return;
  }
  
  for (const [name, preset] of Object.entries(config.presets)) {
    console.log(chalk.bold.white(`  ${name}`));
    console.log(chalk.gray(`    ${preset.description}`));
    
    if (verbose) {
      if (preset.enable.length > 0) {
        console.log(chalk.green(`    Enable: ${preset.enable.join(', ')}`));
      }
      if (preset.disable.length > 0) {
        console.log(chalk.red(`    Disable: ${preset.disable.join(', ')}`));
      }
    }
    console.log('');
  }
}

/**
 * Enable a feature
 */
export async function enableFeature(env: BuildEnvironment, featureName: string): Promise<boolean> {
  const config = await loadFeaturesConfig(env.projectRoot);
  
  for (const category of FEATURE_CATEGORIES) {
    const categoryConfig = config[category] as Record<string, FeatureDefinition> | undefined;
    if (!categoryConfig) continue;
    
    if (categoryConfig[featureName]) {
      const feature = categoryConfig[featureName];
      
      // Check dependencies
      if (feature.dependencies) {
        for (const dep of feature.dependencies) {
          const depEnabled = await isFeatureEnabledInConfig(config, dep);
          if (!depEnabled) {
            console.log(chalk.yellow(`⚠ Feature '${featureName}' depends on '${dep}'. Enabling '${dep}' first...`));
            await enableFeatureInConfig(config, dep);
          }
        }
      }
      
      feature.enabled = true;
      await saveFeaturesConfig(env.projectRoot, config);
      console.log(chalk.green(`✓ Enabled feature: ${featureName}`));
      return true;
    }
  }
  
  console.log(chalk.red(`✗ Feature not found: ${featureName}`));
  return false;
}

/**
 * Disable a feature
 */
export async function disableFeature(env: BuildEnvironment, featureName: string): Promise<boolean> {
  const config = await loadFeaturesConfig(env.projectRoot);
  
  for (const category of FEATURE_CATEGORIES) {
    const categoryConfig = config[category] as Record<string, FeatureDefinition> | undefined;
    if (!categoryConfig) continue;
    
    if (categoryConfig[featureName]) {
      const feature = categoryConfig[featureName];
      
      // Check if required
      if (feature.required) {
        console.log(chalk.red(`✗ Cannot disable required feature: ${featureName}`));
        return false;
      }
      
      // Warn about dependent features
      const dependents = findDependentFeatures(config, featureName);
      if (dependents.length > 0) {
        console.log(chalk.yellow(`⚠ Warning: The following features depend on '${featureName}':`));
        for (const dep of dependents) {
          console.log(chalk.yellow(`   - ${dep}`));
        }
        console.log(chalk.yellow(`   They will be disabled as well.`));
        
        // Disable dependents
        for (const dep of dependents) {
          await disableFeatureInConfig(config, dep);
        }
      }
      
      feature.enabled = false;
      await saveFeaturesConfig(env.projectRoot, config);
      console.log(chalk.green(`✓ Disabled feature: ${featureName}`));
      return true;
    }
  }
  
  console.log(chalk.red(`✗ Feature not found: ${featureName}`));
  return false;
}

/**
 * Toggle a feature
 */
export async function toggleFeature(env: BuildEnvironment, featureName: string): Promise<boolean> {
  const config = await loadFeaturesConfig(env.projectRoot);
  
  for (const category of FEATURE_CATEGORIES) {
    const categoryConfig = config[category] as Record<string, FeatureDefinition> | undefined;
    if (!categoryConfig) continue;
    
    if (categoryConfig[featureName]) {
      if (categoryConfig[featureName].enabled) {
        return await disableFeature(env, featureName);
      } else {
        return await enableFeature(env, featureName);
      }
    }
  }
  
  console.log(chalk.red(`✗ Feature not found: ${featureName}`));
  return false;
}

/**
 * Apply a preset
 */
export async function applyPreset(env: BuildEnvironment, presetName: string): Promise<boolean> {
  const config = await loadFeaturesConfig(env.projectRoot);
  
  if (!config.presets?.[presetName]) {
    console.log(chalk.red(`✗ Preset not found: ${presetName}`));
    console.log(chalk.gray('  Use "features presets" to list available presets'));
    return false;
  }
  
  const preset = config.presets[presetName];
  console.log(chalk.cyan(`\n🎯 Applying preset: ${presetName}`));
  console.log(chalk.gray(`   ${preset.description}\n`));
  
  // Apply enables first
  for (const featureName of preset.enable) {
    await enableFeatureInConfig(config, featureName);
    console.log(chalk.green(`  ✓ Enabled: ${featureName}`));
  }
  
  // Then disables
  for (const featureName of preset.disable) {
    const disabled = await disableFeatureInConfig(config, featureName);
    if (disabled) {
      console.log(chalk.red(`  ✗ Disabled: ${featureName}`));
    }
  }
  
  await saveFeaturesConfig(env.projectRoot, config);
  console.log(chalk.green(`\n✓ Preset '${presetName}' applied successfully`));
  return true;
}

/**
 * Show feature status
 */
export async function showFeature(env: BuildEnvironment, featureName: string): Promise<void> {
  const config = await loadFeaturesConfig(env.projectRoot);
  
  for (const category of FEATURE_CATEGORIES) {
    const categoryConfig = config[category] as Record<string, FeatureDefinition> | undefined;
    if (!categoryConfig) continue;
    
    if (categoryConfig[featureName]) {
      const feature = categoryConfig[featureName];
      const status = feature.enabled ? chalk.green('ENABLED') : chalk.red('DISABLED');
      
      console.log(chalk.bold.cyan(`\n📦 Feature: ${featureName}\n`));
      console.log(`  Status:       ${status}`);
      console.log(`  Category:     ${category}`);
      console.log(`  Description:  ${feature.description}`);
      console.log(`  cfg_flag:     ${feature.cfg_flag}`);
      
      if (feature.required) {
        console.log(`  Required:     ${chalk.magenta('Yes')}`);
      }
      
      if (feature.dependencies?.length) {
        console.log(`  Dependencies: ${feature.dependencies.join(', ')}`);
        
        // Check dependency status
        for (const dep of feature.dependencies) {
          const depEnabled = await isFeatureEnabledInConfig(config, dep);
          const depStatus = depEnabled ? chalk.green('✓') : chalk.red('✗');
          console.log(chalk.gray(`                ${depStatus} ${dep}`));
        }
      }
      
      // Show dependents
      const dependents = findDependentFeatures(config, featureName);
      if (dependents.length > 0) {
        console.log(`  Dependents:   ${dependents.join(', ')}`);
      }
      
      console.log('');
      return;
    }
  }
  
  console.log(chalk.red(`\n✗ Feature not found: ${featureName}\n`));
}

/**
 * Get enabled feature flags for rustc
 */
export async function getEnabledFlags(env: BuildEnvironment): Promise<string[]> {
  const config = await loadFeaturesConfig(env.projectRoot);
  const flags: string[] = [];
  
  for (const category of FEATURE_CATEGORIES) {
    const categoryConfig = config[category] as Record<string, FeatureDefinition> | undefined;
    if (!categoryConfig) continue;
    
    for (const [name, feature] of Object.entries(categoryConfig)) {
      // Check environment variable override
      const envVar = `FEATURE_${name.toUpperCase()}`;
      const envValue = process.env[envVar];
      
      let isEnabled = feature.enabled;
      if (envValue !== undefined) {
        isEnabled = envValue.toLowerCase() === 'true' || envValue === '1';
      }
      
      if (isEnabled) {
        flags.push(feature.cfg_flag);
      }
    }
  }
  
  return flags;
}

/**
 * Print enabled features as RUSTFLAGS
 */
export async function printRustFlags(env: BuildEnvironment): Promise<void> {
  const flags = await getEnabledFlags(env);
  const rustFlags = flags.map(f => `--cfg ${f}`).join(' ');
  
  console.log(chalk.bold.cyan('\n🔧 RUSTFLAGS for kernel build:\n'));
  console.log(chalk.white(rustFlags));
  console.log('');
}

/**
 * Reset all features to default (as defined in features.yaml)
 */
export async function resetFeatures(_env: BuildEnvironment): Promise<void> {
  // Re-read the original config (no modifications)
  // Since we always modify in memory and save, we need to restore defaults
  // For now, we'll just list what would be the defaults
  console.log(chalk.yellow('⚠ To reset features to default, manually edit config/features.yaml'));
  console.log(chalk.gray('  or use git checkout config/features.yaml'));
}

// Helper functions

async function isFeatureEnabledInConfig(config: FeatureFlagsConfig, featureName: string): Promise<boolean> {
  for (const category of FEATURE_CATEGORIES) {
    const categoryConfig = config[category] as Record<string, FeatureDefinition> | undefined;
    if (categoryConfig?.[featureName]) {
      return categoryConfig[featureName].enabled;
    }
  }
  return false;
}

async function enableFeatureInConfig(config: FeatureFlagsConfig, featureName: string): Promise<boolean> {
  for (const category of FEATURE_CATEGORIES) {
    const categoryConfig = config[category] as Record<string, FeatureDefinition> | undefined;
    if (categoryConfig?.[featureName]) {
      categoryConfig[featureName].enabled = true;
      return true;
    }
  }
  return false;
}

async function disableFeatureInConfig(config: FeatureFlagsConfig, featureName: string): Promise<boolean> {
  for (const category of FEATURE_CATEGORIES) {
    const categoryConfig = config[category] as Record<string, FeatureDefinition> | undefined;
    if (categoryConfig?.[featureName]) {
      if (categoryConfig[featureName].required) {
        return false;
      }
      categoryConfig[featureName].enabled = false;
      return true;
    }
  }
  return false;
}

function findDependentFeatures(config: FeatureFlagsConfig, featureName: string): string[] {
  const dependents: string[] = [];
  
  for (const category of FEATURE_CATEGORIES) {
    const categoryConfig = config[category] as Record<string, FeatureDefinition> | undefined;
    if (!categoryConfig) continue;
    
    for (const [name, feature] of Object.entries(categoryConfig)) {
      if (feature.dependencies?.includes(featureName) && feature.enabled) {
        dependents.push(name);
      }
    }
  }
  
  return dependents;
}

/**
 * Interactive feature selection (for future TUI)
 */
export async function interactiveFeatures(_env: BuildEnvironment): Promise<void> {
  console.log(chalk.yellow('⚠ Interactive mode not yet implemented'));
  console.log(chalk.gray('  Use "features list" to see all features'));
  console.log(chalk.gray('  Use "features enable <name>" to enable a feature'));
  console.log(chalk.gray('  Use "features disable <name>" to disable a feature'));
}
