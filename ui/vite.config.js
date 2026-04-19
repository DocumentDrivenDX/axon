import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

const apiTarget = process.env.AXON_API_URL ?? 'http://localhost:4170';

export default defineConfig({
	plugins: [sveltekit()],
	server: {
		proxy: {
			'/auth': apiTarget,
			'/audit': apiTarget,
			'/collections': apiTarget,
			'/control': apiTarget,
			'/entities': apiTarget,
			'/health': apiTarget,
			'/tenants': apiTarget,
		},
	},
});
