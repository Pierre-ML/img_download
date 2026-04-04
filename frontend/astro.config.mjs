import { defineConfig } from 'astro/config';
import tailwind from '@astrojs/tailwind';

export default defineConfig({
  integrations: [tailwind()],
  outDir: '../static',
  // En mode dev, proxy les appels /api vers Flask (port 5000)
  vite: {
    server: {
      proxy: {
        '/api': 'http://localhost:5000',
      },
    },
  },
});
