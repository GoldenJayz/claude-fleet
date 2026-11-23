import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import fs from 'node:fs';
import path from 'node:path';

// When the UI is opened in a plain browser during development (no Tauri), it still needs the
// local collector's token. Read it from the collector's config at dev-server time only;
// production builds never contain it.
function devLocalCollector() {
  try {
    const dir = path.join(process.env.APPDATA ?? '', 'ClaudeFleet', 'collector.json');
    const c = JSON.parse(fs.readFileSync(dir, 'utf8'));
    return { token: c.token as string, port: c.port as number, name: c.machine_name as string };
  } catch {
    return null;
  }
}

export default defineConfig(({ command }) => ({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  define: {
    __DEV_LOCAL__: JSON.stringify(command === 'serve' ? devLocalCollector() : null),
  },
  build: { target: 'es2022', outDir: 'dist' },
}));
