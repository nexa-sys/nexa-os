import axios from 'axios';

const client = axios.create({
  baseURL: 'http://127.0.0.1:8765/api',
  timeout: 60000,
});

export const api = {
  // Features
  async getFeatures() {
    const { data } = await client.get('/features');
    return data;
  },

  async updateFeature(category: string, name: string, enabled: boolean) {
    const { data } = await client.put(`/features/${category}/${name}`, { enabled });
    return data;
  },

  // Modules
  async getModules() {
    const { data } = await client.get('/modules');
    return data;
  },

  async updateModule(name: string, enabled: boolean) {
    const { data } = await client.put(`/modules/${name}`, { enabled });
    return data;
  },

  // Programs
  async getPrograms() {
    const { data } = await client.get('/programs');
    return data;
  },

  async updateProgram(category: string, pkg: string, enabled: boolean) {
    const { data } = await client.put(`/programs/${category}/${pkg}`, { enabled });
    return data;
  },

  // Presets
  async getPresets() {
    const { data } = await client.get('/presets');
    return data;
  },

  async applyPreset(type: 'features' | 'modules', name: string) {
    const { data } = await client.post('/presets/apply', { type, name });
    return data;
  },

  // Estimates
  async getEstimate() {
    const { data } = await client.get('/estimate');
    return data;
  }
};
