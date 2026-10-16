import { defineConfig } from 'vitest/config';

// Tauri expects a fixed dev port and a static build in dist/.
// One page per window: captions (main), reply, script, settings.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: 'es2022',
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: { input: { main: 'index.html', reply: 'reply.html', script: 'script.html', settings: 'settings.html' } },
  },
  test: { include: ['tests/**/*.test.ts'] },
});
